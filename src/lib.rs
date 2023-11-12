#![no_std]
#![cfg_attr(feature = "quiet", allow(warnings))]

mod audio;
mod bitstream;
mod wasm4;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));
use generated::*;

use core::mem::MaybeUninit;

use bitstream::BitStream;

const MOVIE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/movie.bin"));
const RUNS_TREE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runs-tree.bin"));
const RUNS_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runs-data.bin"));

const BPP: u8 = generated::PALETTE.len().trailing_zeros() as u8;

const PIXEL_SIZE: u32 = match (160 / WIDTH, 160 / HEIGHT) {
    (w, h) if w < h => w,
    (_, h) => h,
};

static mut STATE: MaybeUninit<(BitStream, u32, u32, audio::Program)> = MaybeUninit::uninit();

#[no_mangle]
fn start() {
    unsafe {
        *wasm4::SYSTEM_FLAGS =
            wasm4::SYSTEM_PRESERVE_FRAMEBUFFER | wasm4::SYSTEM_HIDE_GAMEPAD_OVERLAY;
        STATE = MaybeUninit::new((BitStream::new(MOVIE), 0, 0, audio::Program::new()));
        // Load palette
        let palette = &mut *wasm4::PALETTE;
        for i in 0..generated::PALETTE.len() {
            palette[i] = PALETTE[i];
        }
        if BPP == 1 {
            palette[3] = (palette[0] * 2 + palette[1]) / 3;
            palette[2] = (palette[0] + palette[1] * 2) / 3;
        }
        (*wasm4::FRAMEBUFFER).fill(0);
    }
}

#[no_mangle]
fn update() {
    let state = unsafe { STATE.assume_init_mut() };

    state.1 += FRAMERATE;

    while state.1 >= 60 {
        state.1 -= 60;
        if state.2 == FRAMECOUNT {
            start();
            return;
        }
        state.2 += 1;
        decode_frame(&mut state.0);
    }

    state.3.update();
}

fn decode_frame(stream: &mut BitStream) {
    if BPP == 1 {
        undo_smooth_filter();
    }

    let mut i = -1;
    for _ in 0..decode_num_rects(|| stream.read_one().unwrap()) {
        i += stream.read_int().unwrap() as i32;
        let (x, y) = get_xy(i as u32, 0, WIDTH, HEIGHT);
        let (tx, ty) = get_xy(i as u32 + stream.read_int().unwrap() - 1, 0, WIDTH, HEIGHT);
        let w = tx - x + 1;
        let h = ty - y + 1;

        decode_rect(stream, x, y, w, h);
    }

    if BPP == 1 {
        apply_smooth_filter();
    }
}

fn decode_rect(stream: &mut BitStream, x: u32, y: u32, w: u32, h: u32) {
    let order = match w == 1 || h == 1 {
        true => 0,
        false => decode_order(|| stream.read_one().unwrap()),
    };

    let mut i = 0;
    while i < w * h {
        let index = huffman_index(stream, RUNS_TREE) * RUN_DATA_SIZE as usize;
        let mut rundata = BitStream::new(&RUNS_DATA[(index / 8)..]);
        rundata.read_bits((index % 8) as u8);
        let rundata = rundata.read_bits(RUN_DATA_SIZE as u8).unwrap();
        let kind = rundata % ((1 << BPP) + 1);
        let length = rundata / ((1 << BPP) + 1);

        if kind == 1 << BPP {
            i += length;
        } else {
            for _ in 0..length {
                let (dx, dy) = get_xy(i, order, w, h);
                set(x + dx, y + dy, kind as u8);
                i += 1;
            }
        }
    }
}

fn huffman_index(stream: &mut BitStream, tree: &[u8]) -> usize {
    let mut tree_stream = BitStream::new(tree);
    let mut value = 0;
    while !tree_stream.read_one().unwrap() {
        if stream.read_one().unwrap() {
            value += count(&mut tree_stream);
        }
    }
    value
}

fn count(tree: &mut BitStream) -> usize {
    if tree.read_one().unwrap() {
        1
    } else {
        count(tree) + count(tree)
    }
}

fn set(x: u32, y: u32, v: u8) {
    unsafe {
        for x in x * PIXEL_SIZE..(x + 1) * PIXEL_SIZE {
            for y in y * PIXEL_SIZE..(y + 1) * PIXEL_SIZE {
                let (i, s) = locate(x, y);
                (*wasm4::FRAMEBUFFER)[i] &= !(0b11 << s);
                (*wasm4::FRAMEBUFFER)[i] |= v << s;
            }
        }
    }
}

fn xor(x: u32, y: u32, v: u8) {
    unsafe {
        for x in x * PIXEL_SIZE..(x + 1) * PIXEL_SIZE {
            for y in y * PIXEL_SIZE..(y + 1) * PIXEL_SIZE {
                let (i, s) = locate(x, y);
                (*wasm4::FRAMEBUFFER)[i] ^= v << s;
            }
        }
    }
}

fn locate(x: u32, y: u32) -> (usize, u32) {
    let offset_y = (160 - HEIGHT * PIXEL_SIZE) / 2;
    let offset_x = (160 - WIDTH * PIXEL_SIZE) / 2;
    let pixel = (y + offset_y) * 160 + x + offset_x;
    let pixel_byte = pixel / 4;
    let pixel_shift = (pixel % 4) * 2;
    (pixel_byte as usize, pixel_shift)
}

fn get_xy(i: u32, order: u32, w: u32, h: u32) -> (u32, u32) {
    if order & 1 == 1 {
        let (y, x) = get_xy(i, order & !1, h, w);
        return (x, y);
    }
    match order {
        0 => (i % w, i / w),
        2 => {
            let y = i / w;
            let x = i % w;
            match y % 2 != 0 {
                false => (x, y),
                true => (w - x - 1, y),
            }
        }
        _ => unreachable!(),
    }
}

#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

fn undo_smooth_filter() {
    unsafe {
        for b in (*wasm4::FRAMEBUFFER).iter_mut() {
            *b &= 0b01010101;
        }
    }
}

fn apply_smooth_filter() {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if x != 0 && y != 0 {
                do_smooth(x * PIXEL_SIZE, y * PIXEL_SIZE, -1, -1);
                if PIXEL_SIZE > 3 {
                    do_smooth(x * PIXEL_SIZE + 1, y * PIXEL_SIZE, -2, -1);
                    do_smooth(x * PIXEL_SIZE, y * PIXEL_SIZE + 1, -1, -2);
                }
            }
            if x != WIDTH - 1 && y != 0 {
                do_smooth((x + 1) * PIXEL_SIZE - 1, y * PIXEL_SIZE, 1, -1);
                if PIXEL_SIZE > 3 {
                    do_smooth((x + 1) * PIXEL_SIZE - 1 - 1, y * PIXEL_SIZE, 2, -1);
                    do_smooth((x + 1) * PIXEL_SIZE - 1, y * PIXEL_SIZE + 1, 1, -2);
                }
            }
            if x != 0 && y != HEIGHT - 1 {
                do_smooth(x * PIXEL_SIZE, (y + 1) * PIXEL_SIZE - 1, -1, 1);
                if PIXEL_SIZE > 3 {
                    do_smooth(x * PIXEL_SIZE + 1, (y + 1) * PIXEL_SIZE - 1, -2, 1);
                    do_smooth(x * PIXEL_SIZE, (y + 1) * PIXEL_SIZE - 1 - 1, -1, 2);
                }
            }
            if x != WIDTH - 1 && y != HEIGHT - 1 {
                do_smooth((x + 1) * PIXEL_SIZE - 1, (y + 1) * PIXEL_SIZE - 1, 1, 1);
                if PIXEL_SIZE > 3 {
                    do_smooth((x + 1) * PIXEL_SIZE - 1 - 1, (y + 1) * PIXEL_SIZE - 1, 2, 1);
                    do_smooth((x + 1) * PIXEL_SIZE - 1, (y + 1) * PIXEL_SIZE - 1 - 1, 1, 2);
                }
            }
        }
    }
}

fn do_smooth(x: u32, y: u32, dx: i32, dy: i32) {
    let (i, s) = locate(x, y);
    let (ix, sx) = locate((x as i32 + dx) as u32, y);
    let (iy, sy) = locate(x, (y as i32 + dy) as u32);
    let fb = unsafe { &mut *wasm4::FRAMEBUFFER };
    let v = fb[i] >> s & 1;
    let vx = fb[ix] >> sx & 1;
    let vy = fb[iy] >> sy & 1;
    if vx == vy && v != vx {
        fb[i] ^= 0b10 << s;
    }
}
