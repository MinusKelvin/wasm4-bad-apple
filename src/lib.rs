#![no_std]
#![cfg_attr(feature = "quiet", allow(warnings))]

mod audio;
mod bitstream;
mod wasm4;

use core::mem::MaybeUninit;

use bitstream::BitStream;

const MOVIE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/movie.bin"));

const FRAMERATE: u32 = MOVIE[0] as u32;
const WIDTH: u32 = MOVIE[1] as u32;
const HEIGHT: u32 = MOVIE[2] as u32;
const FRAME_SIZE: u32 = WIDTH * HEIGHT;
const BPP: u8 = 1;

const PIXEL_SIZE: u32 = match (160 / WIDTH, 160 / HEIGHT) {
    (w, h) if w < h => w,
    (_, h) => h,
};

static mut STATE: MaybeUninit<(BitStream, u32, audio::Program)> = MaybeUninit::uninit();

#[no_mangle]
fn start() {
    unsafe {
        *wasm4::SYSTEM_FLAGS =
            wasm4::SYSTEM_PRESERVE_FRAMEBUFFER | wasm4::SYSTEM_HIDE_GAMEPAD_OVERLAY;
        STATE = MaybeUninit::new((BitStream::new(MOVIE), 0, audio::Program::new()));
        let stream = &mut STATE.assume_init_mut().0;
        // Skip header
        stream.read_bits(24);
        // Load palette
        let palette = &mut *wasm4::PALETTE;
        for i in 0..1 << BPP {
            palette[i] = stream.read_bits(24).unwrap();
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
        if decode_frame(&mut state.0).is_none() {
            start();
            decode_frame(&mut state.0);
        }
    }

    state.2.update();
}

fn decode_frame(stream: &mut BitStream) -> Option<()> {
    if BPP == 1 {
        undo_smooth_filter();
    }

    match stream.read_bits(3)? {
        0 => decode_rle(stream, |i| scanline(i, WIDTH))?,
        1 => decode_rle(stream, |i| transpose(scanline(i, HEIGHT)))?,
        2 => decode_rle(stream, |i| snake(i, WIDTH))?,
        3 => decode_rle(stream, |i| transpose(snake(i, HEIGHT)))?,
        4 => decode_lz77(stream, |i| scanline(i, WIDTH))?,
        5 => decode_lz77(stream, |i| transpose(scanline(i, HEIGHT)))?,
        6 => decode_lz77(stream, |i| snake(i, WIDTH))?,
        7 => decode_lz77(stream, |i| transpose(snake(i, HEIGHT)))?,
        _ => unreachable!(),
    }

    if BPP == 1 {
        apply_smooth_filter();
    }

    Some(())
}

fn decode_rle(stream: &mut BitStream, to_xy: fn(u32) -> (u32, u32)) -> Option<()> {
    let mut i = 0;
    while i < FRAME_SIZE {
        let kind = stream.read_bits(BPP + 1)?;

        if kind > 1 << BPP {
            let length = kind - (1 << BPP) + 1;
            for _ in 0..length {
                let (x, y) = to_xy(i);
                set(x, y, stream.read_bits(BPP)? as u8);
                i += 1;
            }
        } else if kind == 1 << BPP {
            i += stream.read_int()?;
        } else {
            for _ in 0..stream.read_int()? {
                let (x, y) = to_xy(i);
                set(x, y, kind as u8);
                i += 1;
            }
        }
    }
    Some(())
}

fn decode_lz77(stream: &mut BitStream, to_xy: impl Fn(u32) -> (u32, u32)) -> Option<()> {
    let mut i = 0;
    while i < FRAME_SIZE {
        let back = stream.read_int()? - 1;
        let length = stream.read_int()?;
        for _ in 0..length {
            let (x, y) = to_xy(i);
            let v = match back != 0 {
                true => {
                    let read_i = (i + FRAME_SIZE - back) % FRAME_SIZE;
                    let (rx, ry) = to_xy(read_i);
                    get(rx, ry)
                }
                false => stream.read_bits(BPP)? as u8,
            };
            set(x, y, v);
            i += 1;
        }
    }
    Some(())
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

fn get(x: u32, y: u32) -> u8 {
    unsafe {
        let (i, s) = locate(x * PIXEL_SIZE, y * PIXEL_SIZE);
        (*wasm4::FRAMEBUFFER)[i] >> s & 0b11
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

fn scanline(i: u32, width: u32) -> (u32, u32) {
    (i % width, i / width)
}

fn snake(i: u32, width: u32) -> (u32, u32) {
    let y = i / width;
    let x = i % width;
    match y % 2 != 0 {
        false => (x, y),
        true => (width - x - 1, y),
    }
}

fn transpose((y, x): (u32, u32)) -> (u32, u32) {
    (x, y)
}

#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm::unreachable()
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
