use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::process::Command;

use image::imageops::{ColorMap, FilterType};
use image::{imageops, GrayImage, Rgb};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::slice::ParallelSlice;

use crate::bitvec::BitVec;

mod bitvec;

const FRAMERATE: u32 = 10;
const RESCALE_WIDTH: u32 = 40;
const RESCALE_HEIGHT: u32 = 30;
const PALETTE: &[Rgb<u8>] = &[
    Rgb([0x00; 3]),
    // Rgb([0x55; 3]),
    // Rgb([0xAA; 3]),
    Rgb([0xFF; 3]),
];
const START_OFFSET: u32 = 30;
const MAX_FRAMES: u32 = u32::MAX;
const DOWNSCALE_FILTER: FilterType = FilterType::Gaussian;

const BPP: u32 = PALETTE.len().trailing_zeros();
const UNCHANGED_BIT: u32 = 1 << BPP;

struct Palette;

impl ColorMap for Palette {
    type Color = Rgb<u8>;

    fn index_of(&self, color: &Self::Color) -> usize {
        PALETTE
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    i,
                    c.0.iter()
                        .zip(color.0.iter())
                        .map(|(&v1, &v2)| (v1 as i32 - v2 as i32).abs())
                        .sum::<i32>(),
                )
            })
            .min_by_key(|&(_, d)| d)
            .unwrap()
            .0
    }

    fn map_color(&self, color: &mut Self::Color) {
        *color = PALETTE[self.index_of(color)];
    }
}

struct Run {
    length: u32,
    kind: u8,
    extra_data: BitVec,
}

fn main() {
    println!("cargo:rerun-if-changed=frames/");
    println!("cargo:rerun-if-changed=audio.py");
    println!("cargo:rerun-if-changed=music.mid");

    let last_frame = (1..=MAX_FRAMES)
        .take_while(|&i| {
            Path::new(&format!("frames/{}.png", i * 30 / FRAMERATE + START_OFFSET)).is_file()
        })
        .last()
        .unwrap();

    println!("cargo:warning={last_frame} frames");

    let images = (0..=last_frame)
        .into_par_iter()
        .map(|i| match i {
            0 => Ok((
                GrayImage::new(RESCALE_WIDTH, RESCALE_HEIGHT),
                GrayImage::new(RESCALE_HEIGHT, RESCALE_WIDTH),
            )),
            _ => image::open(format!("frames/{}.png", i * 30 / FRAMERATE + START_OFFSET)).map(
                |img| {
                    let smol = imageops::resize(
                        &img.to_rgb8(),
                        RESCALE_WIDTH,
                        RESCALE_HEIGHT,
                        DOWNSCALE_FILTER,
                    );
                    let base = imageops::index_colors(&smol, &Palette);
                    let mut transposed = GrayImage::new(base.height(), base.width());
                    for y in 0..transposed.height() {
                        for x in 0..transposed.width() {
                            transposed.put_pixel(x, y, *base.get_pixel(y, x));
                        }
                    }
                    (base, transposed)
                },
            ),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let data: Vec<_> = images
        .par_windows(2)
        .map(|v| {
            let (prev_h, prev_v) = &v[0];
            let (curr_h, curr_v) = &v[1];
            [
                encode(value_sets(scanline(curr_h), scanline(prev_h))),
                encode(value_sets(scanline(curr_v), scanline(prev_v))),
                encode(value_sets(snake(curr_h), snake(prev_h))),
                encode(value_sets(snake(curr_v), snake(prev_v))),
                lz77(scanline(prev_h).chain(scanline(curr_h))),
                lz77(scanline(prev_v).chain(scanline(curr_v))),
                lz77(snake(prev_h).chain(snake(curr_h))),
                lz77(snake(prev_v).chain(snake(curr_v))),
            ]
            .into_iter()
            .enumerate()
            .min_by_key(|(_, data)| data.len())
            .unwrap()
        })
        .collect();

    let mut movie = BitVec::new();
    movie.write_bits(FRAMERATE, 8);
    movie.write_bits(RESCALE_WIDTH, 8);
    movie.write_bits(RESCALE_HEIGHT, 8);
    for color in PALETTE {
        for &channel in color.0.iter().rev() {
            movie.write_bits(channel as u32, 8);
        }
    }

    let mut frame_variants = [0; 8];
    for (variant, frame) in data {
        movie.write_bits(variant as u32, 3);
        movie.append(frame);
        frame_variants[variant as usize] += 1;
    }

    println!("cargo:warning=Frame variants {:?}", frame_variants);
    println!("cargo:warning=Movie size: {}", (movie.len() + 7) / 8);

    movie
        .dump(BufWriter::new(
            File::create(format!("{}/movie.bin", env::var("OUT_DIR").unwrap())).unwrap(),
        ))
        .unwrap();

    assert!(Command::new("./audio.py").status().unwrap().success());
}

fn encode(mut value_sets: impl Iterator<Item = u8>) -> BitVec {
    let mut data = vec![];

    let mut length = 1;
    let mut current = value_sets.next().unwrap();

    for next in value_sets {
        if next & current == 0 {
            data.push(Run {
                length,
                kind: current.trailing_zeros() as u8,
                extra_data: BitVec::new(),
            });
            length = 1;
            current = next;
        } else {
            length += 1;
            current &= next;
        }
    }
    data.push(Run {
        length,
        kind: current.trailing_zeros() as u8,
        extra_data: BitVec::new(),
    });

    data.dedup_by(|next, prev| {
        if prev.length + next.length <= UNCHANGED_BIT && next.kind < UNCHANGED_BIT as u8 {
            if prev.kind < UNCHANGED_BIT as u8 {
                for _ in 0..prev.length {
                    prev.extra_data.write_bits(prev.kind as u32, BPP);
                }
                for _ in 0..next.length {
                    prev.extra_data.write_bits(next.kind as u32, BPP);
                }
                prev.length += next.length;
                prev.kind = (UNCHANGED_BIT + prev.length - 1) as u8;
                return true;
            } else if prev.kind > UNCHANGED_BIT as u8 {
                for _ in 0..next.length {
                    prev.extra_data.write_bits(next.kind as u32, BPP);
                }
                prev.kind += next.length as u8;
                prev.length += next.length;
                return true;
            }
        }
        false
    });

    let mut encoded = BitVec::new();
    for run in data {
        encoded.write_bits(run.kind as u32, BPP + 1);
        if run.kind > UNCHANGED_BIT as u8 {
            encoded.append(run.extra_data);
        } else {
            encoded.write_int(run.length);
        }
    }

    encoded
}

fn value_sets(
    current: impl Iterator<Item = u8>,
    previous: impl Iterator<Item = u8>,
) -> impl Iterator<Item = u8> {
    current
        .zip(previous)
        .map(|(c, p)| 1 << c | ((c == p) as u8) << UNCHANGED_BIT)
}

fn scanline(img: &GrayImage) -> impl Iterator<Item = u8> + '_ {
    img.pixels().map(|p| p.0[0])
}

fn snake(img: &GrayImage) -> impl Iterator<Item = u8> + '_ {
    (0..img.height()).flat_map(move |y| {
        (0..img.width()).map(move |x| match y % 2 != 0 {
            false => img.get_pixel(x, y).0[0],
            true => img.get_pixel(img.width() - x - 1, y).0[0],
        })
    })
}

fn lz77(pixels: impl Iterator<Item = u8>) -> BitVec {
    let pixels: Vec<_> = pixels.collect();
    let frame_length = pixels.len() / 2;
    let mut parts = vec![];
    let mut i = pixels.len() / 2;

    while i < pixels.len() {
        let (length, backwards) = find_longest_match(&pixels[i - frame_length..], frame_length);
        if backwards != 0 {
            let mut backref = BitVec::new();
            backref.write_int(backwards as u32 + 1);
            backref.write_int(length as u32);
            let mut raw = BitVec::new();
            raw.write_int(1);
            raw.write_int(length as u32);
            for &v in &pixels[i..i + length] {
                raw.write_bits(v as u32, BPP);
            }
            if backref.len() < raw.len() {
                parts.push(Ok(backref));
                i += length;
                continue;
            }
        }

        parts.push(Err(vec![pixels[i]]));
        i += 1;
    }

    parts.dedup_by(|next, curr| match (next, curr) {
        (Err(next), Err(current)) => {
            current.append(next);
            true
        }
        _ => false,
    });

    let mut encoded = BitVec::new();
    for part in parts {
        match part {
            Ok(backref) => encoded.append(backref),
            Err(pixels) => {
                encoded.write_int(1);
                encoded.write_int(pixels.len() as u32);
                for pixel in pixels {
                    encoded.write_bits(pixel as u32, BPP);
                }
            }
        }
    }

    encoded
}

fn find_longest_match(data: &[u8], at: usize) -> (usize, usize) {
    let mut longest_match = (0, 0);
    for i in (0..at).rev() {
        let length = data[i..]
            .iter()
            .zip(data[at..].iter())
            .take_while(|(&a, &b)| a == b)
            .count();
        if length > longest_match.0 {
            longest_match = (length, at - i);
        }
    }
    longest_match
}
