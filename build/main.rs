use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::process::Command;

use image::imageops::ColorMap;
use image::{imageops, GrayImage, Rgb};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::slice::ParallelSlice;

use crate::bitvec::BitVec;

mod bitvec;

const FRAMERATE: u32 = 8;
const RESCALE_WIDTH: u32 = 32;
const RESCALE_HEIGHT: u32 = 24;
const PALETTE: &[Rgb<u8>] = &[
    // Rgb([0x23, 0x0b, 0x03]),
    // Rgb([0xa2, 0x2e, 0x0d]),
    // Rgb([0xe2, 0x62, 0x30]),
    // Rgb([0xfe, 0xff, 0x90]),
    Rgb([0x00; 3]),
    // Rgb([0x55; 3]),
    // Rgb([0xAA; 3]),
    Rgb([0xFF; 3]),
];
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

    let last_frame = (1..)
        .take_while(|&i| Path::new(&format!("frames/{}.png", i * 30 / FRAMERATE + 31)).is_file())
        .last()
        .unwrap();

    let images = (0..=last_frame)
        .into_par_iter()
        .map(|i| match i {
            0 => Ok(GrayImage::new(RESCALE_WIDTH, RESCALE_HEIGHT)),
            _ => image::open(format!("frames/{}.png", i * 30 / FRAMERATE + 31)).map(|img| {
                let smol = imageops::resize(
                    &img.to_rgb8(),
                    RESCALE_WIDTH,
                    RESCALE_HEIGHT,
                    imageops::FilterType::Gaussian,
                );
                imageops::index_colors(&smol, &Palette)
            }),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let data: Vec<_> = images
        .par_windows(2)
        .map(|v| {
            let prev_img = &v[0];
            let img = &v[1];
            let hdata = encode(value_sets(horizontal(img), horizontal(prev_img)));
            let vdata = encode(value_sets(vertical(img), vertical(prev_img)));
            if vdata.len() < hdata.len() {
                (true, vdata)
            } else {
                (false, hdata)
            }
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

    let mut orderings = [0; 2];
    let mut kinds = [0; 8];
    let mut lengths = [0; (RESCALE_WIDTH * RESCALE_HEIGHT) as usize];
    for (vertical, runs) in data {
        movie.write(vertical);
        orderings[vertical as usize] += 1;
        for run in runs {
            movie.write_bits(run.kind as u32, BPP + 1);
            if run.kind > UNCHANGED_BIT as u8 {
                movie.append(run.extra_data);
            } else {
                movie.write_int(run.length);
                lengths[run.length as usize - 1] += 1;
            }
            kinds[run.kind as usize] += 1;
        }
    }

    println!("cargo:warning=Orderings {:?}", orderings);
    println!("cargo:warning=Kinds {:?}", kinds);
    println!("cargo:warning=Lengths {:?}", lengths);

    movie
        .dump(BufWriter::new(
            File::create(format!("{}/movie.bin", env::var("OUT_DIR").unwrap())).unwrap(),
        ))
        .unwrap();

    assert!(Command::new("./audio.py").status().unwrap().success());
}

fn encode(mut value_sets: impl Iterator<Item = u8>) -> Vec<Run> {
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

    data
}

fn value_sets(
    current: impl Iterator<Item = u8>,
    previous: impl Iterator<Item = u8>,
) -> impl Iterator<Item = u8> {
    current
        .zip(previous)
        .map(|(c, p)| 1 << c | ((c == p) as u8) << UNCHANGED_BIT)
}

fn horizontal(img: &GrayImage) -> impl Iterator<Item = u8> + '_ {
    img.pixels().map(|p| p.0[0])
}

fn vertical(img: &GrayImage) -> impl Iterator<Item = u8> + '_ {
    (0..img.width()).flat_map(move |x| (0..img.height()).map(move |y| img.get_pixel(x, y).0[0]))
}
