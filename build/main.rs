use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::Command;

use image::imageops::{ColorMap, FilterType};
use image::{imageops, GrayImage, Rgb};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::slice::ParallelSlice;

use crate::bitvec::BitVec;
use crate::huffman::HuffmanCode;

mod bitvec;
mod huffman;

const FRAMERATE: u32 = 7;
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

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
struct Run {
    length: u32,
    kind: u8,
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    fn xs(self) -> impl Iterator<Item = u32> {
        self.x..self.x + self.w
    }

    fn ys(self) -> impl Iterator<Item = u32> {
        self.y..self.y + self.h
    }
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

    println!("cargo:warning=Frames {}", last_frame);

    let images = (0..=last_frame)
        .into_par_iter()
        .map(|i| match i {
            0 => Ok(GrayImage::new(RESCALE_WIDTH, RESCALE_HEIGHT)),
            _ => image::open(format!("frames/{}.png", i * 30 / FRAMERATE + START_OFFSET)).map(
                |img| {
                    let smol = imageops::resize(
                        &img.to_rgb8(),
                        RESCALE_WIDTH,
                        RESCALE_HEIGHT,
                        DOWNSCALE_FILTER,
                    );
                    imageops::index_colors(&smol, &Palette)
                },
            ),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let data: Vec<_> = images
        .par_windows(2)
        .map(|v| encode_frame(&v[1], &v[0]))
        .collect();

    let mut run_freq = HashMap::new();
    let mut orderings = [0; 4];
    let mut num_rects = vec![];
    let mut biggest_run = 0;
    for rects in &data {
        for &(rect, order, ref runs) in rects {
            if rect.h != 1 && rect.w != 1 {
                orderings[order] += 1;
            }
            for &run in runs {
                *run_freq.entry(run).or_default() += 1;
                let encoded = run.kind as u32 + run.length * (PALETTE.len() as u32 + 1);
                if encoded > biggest_run {
                    biggest_run = encoded;
                }
            }
        }
        while rects.len() >= num_rects.len() {
            num_rects.push(0);
        }
        num_rects[rects.len()] += 1;
    }

    let mut kind_freq = [0; 5];
    for run in run_freq.keys() {
        kind_freq[run.kind as usize] += 1;
    }

    let runs_huffman = HuffmanCode::new(run_freq);

    let (structure, values) = runs_huffman.structure();
    let run_value_bits = (biggest_run + 1).next_power_of_two().trailing_zeros();

    let mut runs_data = BitVec::new();
    for run in values {
        runs_data.write_bits(
            run.kind as u32 + run.length * (PALETTE.len() as u32 + 1),
            run_value_bits,
        );
    }

    let order_huffman = HuffmanCode::new(orderings.into_iter().enumerate().filter(|&(_, v)| v > 0));

    let num_rects_huffman =
        HuffmanCode::new(num_rects.into_iter().enumerate().filter(|&(_, v)| v > 0));

    let frames = data.len();
    let mut movie = BitVec::new();
    for rects in data {
        num_rects_huffman.encode_value(&mut movie, &rects.len());
        let mut last_index = -1;
        for (rect, order, runs) in rects {
            let i = rect.y * RESCALE_WIDTH + rect.x;
            let br = (rect.y + rect.h - 1) * RESCALE_WIDTH + rect.x + rect.w;
            movie.write_int((i as i32 - last_index) as u32);
            movie.write_int(br - i);
            last_index = i as i32;

            if rect.h != 1 && rect.w != 1 {
                order_huffman.encode_value(&mut movie, &order);
            }
            for run in runs {
                runs_huffman.encode_value(&mut movie, &run);
            }
        }
    }

    println!("cargo:warning=Frames {frames}");
    println!(
        "cargo:warning=Movie size {}",
        movie.bytes() + structure.bytes() + runs_data.bytes()
    );

    movie
        .dump(BufWriter::new(
            File::create(format!("{}/movie.bin", env::var("OUT_DIR").unwrap())).unwrap(),
        ))
        .unwrap();

    structure
        .dump(BufWriter::new(
            File::create(format!("{}/runs-tree.bin", env::var("OUT_DIR").unwrap())).unwrap(),
        ))
        .unwrap();

    runs_data
        .dump(BufWriter::new(
            File::create(format!("{}/runs-data.bin", env::var("OUT_DIR").unwrap())).unwrap(),
        ))
        .unwrap();

    let mut code_file = BufWriter::new(
        File::create(format!("{}/generated.rs", env::var("OUT_DIR").unwrap())).unwrap(),
    );

    write!(
        code_file,
        "mod generated {{
        pub const WIDTH: u32 = {RESCALE_WIDTH};
        pub const HEIGHT: u32 = {RESCALE_HEIGHT};
        pub const FRAMECOUNT: u32 = {frames};
        pub const FRAMERATE: u32 = {FRAMERATE};
        pub const RUN_DATA_SIZE: u32 = {run_value_bits};"
    )
    .unwrap();

    order_huffman
        .emit_decoder(&mut code_file, "decode_order", "u32", |to, order| {
            write!(to, "{order}")
        })
        .unwrap();
    num_rects_huffman
        .emit_decoder(&mut code_file, "decode_num_rects", "u32", |to, order| {
            write!(to, "{order}")
        })
        .unwrap();

    write!(code_file, "pub const PALETTE: [u32; {}] = [", PALETTE.len()).unwrap();
    for color in PALETTE {
        write!(code_file, "0x").unwrap();
        for &channel in color.0.iter().rev() {
            write!(code_file, "{:X}", channel).unwrap();
        }
        write!(code_file, ",").unwrap();
    }
    write!(code_file, "];").unwrap();

    write!(code_file, "}}").unwrap();

    assert!(Command::new("./audio.py").status().unwrap().success());
}

fn encode_frame(curr: &GrayImage, prev: &GrayImage) -> Vec<(Rect, usize, Vec<Run>)> {
    let mut rects: Vec<_> = bounding_rect(
        curr,
        prev,
        Rect {
            x: 0,
            y: 0,
            w: curr.width(),
            h: curr.height(),
        },
    )
    .into_iter()
    .map(|rect| encode_rect(curr, prev, rect))
    .collect();

    'split: loop {
        for enc_rect in &mut rects {
            'next: for x in enc_rect.0.xs().skip(1) {
                for y in enc_rect.0.ys() {
                    let left_is_diff = curr.get_pixel(x - 1, y) != prev.get_pixel(x - 1, y);
                    let right_is_diff = curr.get_pixel(x, y) != prev.get_pixel(x, y);
                    if left_is_diff && right_is_diff {
                        continue 'next;
                    }
                }

                let r1 = Rect {
                    w: x - enc_rect.0.x,
                    ..enc_rect.0
                };
                let r1 = encode_rect(curr, prev, bounding_rect(curr, prev, r1).unwrap());

                let r2 = Rect {
                    x,
                    w: enc_rect.0.w - (x - enc_rect.0.x),
                    ..enc_rect.0
                };
                let r2 = encode_rect(curr, prev, bounding_rect(curr, prev, r2).unwrap());

                if r1.2.len() + r2.2.len() + 2 < enc_rect.2.len() {
                    *enc_rect = r1;
                    rects.push(r2);
                    continue 'split;
                }
            }

            'next: for y in enc_rect.0.ys().skip(1) {
                for x in enc_rect.0.xs() {
                    let up_is_diff = curr.get_pixel(x, y - 1) != prev.get_pixel(x, y - 1);
                    let down_is_diff = curr.get_pixel(x, y) != prev.get_pixel(x, y);
                    if up_is_diff && down_is_diff {
                        continue 'next;
                    }
                }

                let r1 = Rect {
                    h: y - enc_rect.0.y,
                    ..enc_rect.0
                };
                let r1 = encode_rect(curr, prev, bounding_rect(curr, prev, r1).unwrap());

                let r2 = Rect {
                    y,
                    h: enc_rect.0.h - (y - enc_rect.0.y),
                    ..enc_rect.0
                };
                let r2 = encode_rect(curr, prev, bounding_rect(curr, prev, r2).unwrap());

                if r1.2.len() + r2.2.len() + 2 < enc_rect.2.len() {
                    *enc_rect = r1;
                    rects.push(r2);
                    continue 'split;
                }
            }
        }
        break;
    }

    rects.sort_by_key(|(r, _, _)| (r.y, r.x));
    rects
}

fn encode_rect(curr: &GrayImage, prev: &GrayImage, rect: Rect) -> (Rect, usize, Vec<Run>) {
    [
        encode(value_sets(curr, prev, scanline(rect))),
        encode(value_sets(curr, prev, transpose(scanline, rect))),
        encode(value_sets(curr, prev, snake(rect))),
        encode(value_sets(curr, prev, transpose(snake, rect))),
    ]
    .into_iter()
    .enumerate()
    .min_by_key(|(_, data)| data.len())
    .map(|(order, data)| (rect, order, data))
    .unwrap()
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
    });

    data
}

fn value_sets<'a>(
    current: &'a GrayImage,
    previous: &'a GrayImage,
    order: impl Iterator<Item = (u32, u32)> + 'a,
) -> impl Iterator<Item = u8> + 'a {
    order.map(move |(x, y)| {
        let c = current.get_pixel(x, y).0[0];
        let p = previous.get_pixel(x, y).0[0];
        1 << c | ((c == p) as u8) << UNCHANGED_BIT
    })
}

fn scanline(rect: Rect) -> impl Iterator<Item = (u32, u32)> {
    rect.ys().flat_map(move |y| rect.xs().map(move |x| (x, y)))
}

fn snake(rect: Rect) -> impl Iterator<Item = (u32, u32)> {
    let mut going_back = true;
    rect.ys().flat_map(move |y| {
        going_back ^= true;
        (0..rect.w).map(move |dx| match going_back {
            false => (rect.x + dx, y),
            true => (rect.x + rect.w - dx - 1, y),
        })
    })
}

fn transpose<I: Iterator<Item = (u32, u32)>>(
    orderer: impl Fn(Rect) -> I,
    rect: Rect,
) -> impl Iterator<Item = (u32, u32)> {
    orderer(Rect {
        x: rect.y,
        y: rect.x,
        w: rect.h,
        h: rect.w,
    })
    .map(|(y, x)| (x, y))
}

fn bounding_rect(curr: &GrayImage, prev: &GrayImage, start: Rect) -> Option<Rect> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0;
    let mut max_y = 0;
    for y in start.ys() {
        for x in start.xs() {
            if curr.get_pixel(x, y) != prev.get_pixel(x, y) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if min_x > max_x {
        None
    } else {
        Some(Rect {
            x: min_x,
            y: min_y,
            w: max_x - min_x + 1,
            h: max_y - min_y + 1,
        })
    }
}
