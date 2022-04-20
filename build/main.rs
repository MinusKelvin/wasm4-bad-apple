use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::imageops::ColorMap;
use image::{imageops, GenericImageView, GrayImage, Luma, Rgb};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::slice::ParallelSlice;

use crate::bitvec::BitVec;

mod bitvec;

const FRAMERATE: u32 = 30;
const RESCALE_WIDTH: u32 = 160;
const RESCALE_HEIGHT: u32 = 120;
const PALETTE: &[Rgb<u8>] = &[
    // Rgb([0x23, 0x0b, 0x03]),
    // Rgb([0xa2, 0x2e, 0x0d]),
    // Rgb([0xe2, 0x62, 0x30]),
    // Rgb([0xfe, 0xff, 0x90]),
    Rgb([0x00; 3]),
    Rgb([0x55; 3]),
    Rgb([0xAA; 3]),
    Rgb([0xFF; 3]),
];
const BPP: u32 = PALETTE.len().trailing_zeros();

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

struct EncodingRect {
    x: u32,
    y: u32,
    code: u32,
    data: BitVec,
}

fn main() {
    println!("cargo:rerun-if-changed=frames");

    let freq = [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ];
    let color_freq = [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ];

    let last_frame = (1..)
        .take_while(|&i| Path::new(&format!("frames/{}.png", i * 30 / FRAMERATE)).is_file())
        .last()
        .unwrap();

    let images = (0..=last_frame)
        .into_par_iter()
        .map(|i| match i {
            0 => Ok(GrayImage::new(RESCALE_WIDTH, RESCALE_HEIGHT)),
            _ => image::open(format!("frames/{}.png", i * 30 / FRAMERATE)).map(|img| {
                let smol = imageops::resize(
                    &img.to_rgb8(),
                    RESCALE_WIDTH,
                    RESCALE_HEIGHT,
                    imageops::FilterType::Nearest,
                );
                imageops::index_colors(&smol, &Palette)
            }),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // let mut stuff = vec![GrayImage::new(RESCALE_WIDTH, RESCALE_HEIGHT)];
    // for imgs in images.windows(2) {
    //     let p = &imgs[0];
    //     let c = &imgs[1];
    //     let mut diff = GrayImage::new(RESCALE_WIDTH, RESCALE_HEIGHT);
    //     for ((v, p), c) in diff.pixels_mut().zip(p.pixels()).zip(c.pixels()) {
    //         *v = Luma([(p != c) as u8 * 3]);
    //     }
    //     stuff.push(diff);
    // }
    // let images = stuff;

    let data: Vec<_> = images
        .par_windows(2)
        .map(|v| {
            let prev_img = &v[0];
            let img = &v[1];
            encode_frame(&img, prev_img, &freq, &color_freq)
        })
        .collect();

    println!("cargo:warning={:?}", freq);

    let mut movie = BitVec::new();
    movie.write_bits(FRAMERATE, 8);
    movie.write_bits(RESCALE_WIDTH, 8);
    movie.write_bits(RESCALE_HEIGHT, 8);
    for color in PALETTE {
        for &channel in color.0.iter().rev() {
            movie.write_bits(channel as u32, 8);
        }
    }
    for bits in data {
        movie.append(bits);
    }
    movie
        .dump(BufWriter::new(
            File::create(format!("{}/movie.bin", env::var("OUT_DIR").unwrap())).unwrap(),
        ))
        .unwrap();
}

fn encode_frame(
    img: &GrayImage,
    prev_img: &GrayImage,
    freq: &[AtomicUsize],
    color_freq: &[AtomicUsize],
) -> BitVec {
    let mut frame = BitVec::new();

    let mut diffs = GrayImage::new(img.width(), img.height());

    for ((&c, &p), d) in img.pixels().zip(prev_img.pixels()).zip(diffs.pixels_mut()) {
        if c != p {
            d.0[0] = 1;
        }
    }

    let mut rects = vec![];
    for y in 0..img.height() {
        for mut x in 0..img.width() {
            if diffs.get_pixel(x, y).0[0] == 1 {
                let mut w = 1;
                let mut h = 1;
                loop {
                    let occupancy = (y..y + h)
                        .flat_map(|y| {
                            let diffs = &diffs;
                            (x..x + w).filter(move |&x| diffs.get_pixel(x, y) == &Luma([1]))
                        })
                        .count() as u32;
                    assert_ne!(occupancy, 0);
                    if 5 * occupancy < w * h {
                        break;
                    }

                    let mut improved = false;
                    let mut check = |xs: Range<u32>, ys: Range<u32>| {
                        let mut expand = false;
                        for y in ys {
                            for x in xs.clone() {
                                match diffs.get_pixel_checked(x, y) {
                                    Some(&Luma([1])) => expand = true,
                                    Some(&Luma([2])) => return false,
                                    _ => {}
                                }
                            }
                        }
                        improved |= expand;
                        expand
                    };
                    if x != 0 && check(x - 1..x, y..y + h + 1) {
                        x -= 1;
                        w += 1;
                    }
                    w += check(x + w..x + w + 1, y..y + h + 1) as u32;
                    h += check(x..x + w, y + h..y + h + 1) as u32;
                    if !improved {
                        break;
                    }
                }

                for y in y..y + h {
                    for x in x..x + w {
                        diffs.put_pixel(x, y, Luma([2]));
                    }
                }

                let (code, data) = encode_rect(
                    &img.view(x, y, w, h).to_image(),
                    &prev_img.view(x, y, w, h).to_image(),
                    color_freq,
                );
                rects.push(EncodingRect { x, y, code, data });
            }
        }
    }
    rects.sort_by_key(|r| (r.y, r.x));

    let mut last = -1;
    for rect in rects {
        let i = rect.y * RESCALE_WIDTH + rect.x;
        frame.write_elias_delta((i as i32 - last) as u32);
        last = i as i32;

        frame.append(rect.data);
        freq[rect.code as usize].fetch_add(1, Ordering::Relaxed);
    }

    frame.write_elias_delta(((RESCALE_WIDTH * RESCALE_HEIGHT) as i32 - last) as u32);
    frame
}

fn encode_rect(
    view: &GrayImage,
    view_prev: &GrayImage,
    color_freq: &[AtomicUsize],
) -> (u32, BitVec) {
    let mut frame = BitVec::new();
    frame.write_elias_delta(view.width());
    frame.write_elias_delta(view.height());

    let len = view.width() * view.height();

    let (num, (code, code_bits, (bits, c_freq))) = [
        (0b10, 2, rle(horizontal(&view), BPP)),
        (0b100, 3, rle(vertical(&view), BPP)),
        (
            0b10000,
            5,
            rle(xor(horizontal(&view), horizontal(&view_prev)), BPP),
        ),
        (
            0b00000,
            5,
            rle(xor(vertical(&view), vertical(&view_prev)), BPP),
        ),
        (
            0b11000,
            5,
            diff_list(horizontal(&view), horizontal(&view_prev), len, BPP),
        ),
        (
            0b01000,
            5,
            diff_list(vertical(&view), vertical(&view_prev), len, BPP),
        ),
        (0b1, 1, direct(horizontal(&view), BPP)),
    ]
    .into_iter()
    .enumerate()
    .min_by_key(|v| v.1 .2 .0.len())
    .unwrap();

    frame.write_bits(code as u32, code_bits);
    frame.append(bits);
    for i in 0..4 {
        color_freq[i].fetch_add(c_freq[i], Ordering::Relaxed);
    }

    (num as u32, frame)
}

fn encode_runs<T: Eq>(mut values: impl Iterator<Item = T>) -> impl Iterator<Item = (T, u32)> {
    let mut run_value = values.next();
    let mut run_length = 1;
    std::iter::from_fn(move || loop {
        if run_value.is_none() {
            return None;
        }

        let next = values.next();
        if run_value == next {
            run_length += 1;
            continue;
        }

        let run = (run_value.take().unwrap(), run_length);
        run_value = next;
        run_length = 1;
        return Some(run);
    })
}

fn horizontal(img: &GrayImage) -> impl Iterator<Item = u8> + '_ {
    img.pixels().map(|p| p.0[0])
}

fn vertical(img: &GrayImage) -> impl Iterator<Item = u8> + '_ {
    (0..img.width()).flat_map(move |x| (0..img.height()).map(move |y| img.get_pixel(x, y).0[0]))
}

fn rle(values: impl Iterator<Item = u8>, value_size: u32) -> (BitVec, [usize; 4]) {
    let mut result = BitVec::new();
    let mut color_freq = [0; 4];
    for (v, run_length) in encode_runs(values) {
        result.write_elias_delta(run_length);
        result.write_bits(v as u32, value_size);
        color_freq[v as usize] += 1;
    }
    (result, color_freq)
}

fn xor(
    current: impl Iterator<Item = u8>,
    prev: impl Iterator<Item = u8>,
) -> impl Iterator<Item = u8> {
    current.zip(prev).map(|(a, b)| a ^ b)
}

fn diff_list(
    current: impl Iterator<Item = u8>,
    prev: impl Iterator<Item = u8>,
    len: u32,
    data_size: u32,
) -> (BitVec, [usize; 4]) {
    let mut diffs = BitVec::new();
    let mut color_freq = [0; 4];
    let mut last = -1;
    for (i, (c, p)) in current.zip(prev).enumerate() {
        if c != p {
            diffs.write_elias_delta((i as i32 - last) as u32);
            color_freq[c as usize] += 1;
            diffs.write_bits(c as u32, data_size);
            last = i as i32;
        }
    }
    diffs.write_elias_delta((len as i32 - last) as u32);
    (diffs, color_freq)
}

fn direct(values: impl Iterator<Item = u8>, data_size: u32) -> (BitVec, [usize; 4]) {
    let mut data = BitVec::new();
    let mut color_freq = [0; 4];
    for v in values {
        color_freq[v as usize] += 1;
        data.write_bits(v as u32, data_size);
    }
    (data, color_freq)
}
