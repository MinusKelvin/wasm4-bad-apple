#![no_std]
#![cfg_attr(feature = "quiet", allow(warnings))]

mod wasm4;

struct BitReader {
    from: &'static [u8],
    current: u8,
    current_bit: u8,
}

impl BitReader {
    fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut bits = 0;
        for i in 0..count {
            bits |= (self.read_one()? as u32) << i;
        }
        Some(bits)
    }

    fn read_one(&mut self) -> Option<bool> {
        if self.current_bit == 8 {
            let (&next, rest) = self.from.split_first()?;
            self.current = next;
            self.from = rest;
            self.current_bit = 0;
        }
        let result = self.current & 1 << self.current_bit != 0;
        self.current_bit += 1;
        Some(result)
    }

    fn read_unary(&mut self) -> Option<u32> {
        let mut v = 0;
        while !self.read_one()? {
            v += 1
        }
        Some(v)
    }

    fn read_elias_gamma(&mut self) -> Option<u32> {
        let l = self.read_unary()? as u8;
        let v = self.read_bits(l)?;
        Some(v | 1 << l)
    }

    fn read_elias_delta(&mut self) -> Option<u32> {
        let l = self.read_elias_gamma()? as u8 - 1;
        let v = self.read_bits(l)?;
        Some(v | 1 << l)
    }
}

const MOVIE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/movie.bin"));
const FRAMERATE: u32 = MOVIE[0] as u32;
const WIDTH: u32 = MOVIE[1] as u32;
const HEIGHT: u32 = MOVIE[2] as u32;
const BPP: u8 = 2;

const PIXEL_SIZE: u32 = match (160 / WIDTH, 160 / HEIGHT) {
    (w, h) if w < h => w,
    (_, h) => h,
};

static mut STATE: (BitReader, u32) = (
    BitReader {
        from: MOVIE,
        current: 0,
        current_bit: 8,
    },
    0,
);

#[no_mangle]
fn start() {
    unsafe {
        *wasm4::SYSTEM_FLAGS =
            wasm4::SYSTEM_PRESERVE_FRAMEBUFFER | wasm4::SYSTEM_HIDE_GAMEPAD_OVERLAY;
        // Skip header
        STATE.0.read_bits(24);
        // Load palette
        *wasm4::PALETTE = [
            STATE.0.read_bits(24).unwrap(),
            STATE.0.read_bits(24).unwrap(),
            STATE.0.read_bits(24).unwrap(),
            STATE.0.read_bits(24).unwrap(),
        ];
        (*wasm4::FRAMEBUFFER).fill(0);
    }
}

#[no_mangle]
fn update() {
    let state = unsafe { &mut STATE };

    state.1 += FRAMERATE;

    while state.1 >= 60 {
        state.1 -= 60;
        if decode_frame(&mut state.0).is_none() {
            unsafe {
                STATE = (
                    BitReader {
                        from: MOVIE,
                        current: 0,
                        current_bit: 8,
                    },
                    0,
                );
            }
            start();
            decode_frame(&mut state.0);
        }
    }
}

fn decode_frame(stream: &mut BitReader) -> Option<()> {
    let mut i = stream.read_elias_delta()? - 1;
    while i < WIDTH * HEIGHT {
        let (x, y) = (i % WIDTH, i / WIDTH);
        let w = stream.read_elias_delta()?;
        let h = stream.read_elias_delta()?;

        let coder = if stream.read_one()? {
            6
        } else if stream.read_one()? {
            0
        } else if stream.read_one()? {
            1
        } else if stream.read_one()? {
            if stream.read_one()? {
                4
            } else {
                5
            }
        } else if stream.read_one()? {
            2
        } else {
            3
        };

        match coder {
            0 => rle_rect(stream, false, w, h, |dx, dy, p| set(x + dx, y + dy, p)),
            1 => rle_rect(stream, true, w, h, |dx, dy, p| set(x + dx, y + dy, p)),
            2 => rle_rect(stream, false, w, h, |dx, dy, p| xor(x + dx, y + dy, p)),
            3 => rle_rect(stream, true, w, h, |dx, dy, p| xor(x + dx, y + dy, p)),
            4 => difflist(stream, false, x, y, w, h).unwrap(),
            5 => difflist(stream, true, x, y, w, h).unwrap(),
            6 => {
                for i in 0..w * h {
                    let (dx, dy) = (i % w, i / w);
                    set(x + dx, y + dy, stream.read_bits(BPP)? as u8);
                }
            }
            _ => unreachable!(),
        }

        // for y in y..y+h {
        //     for x in x..x+w {
        //         set(x, y, 3);
        //     }
        // }

        i += stream.read_elias_delta()?;
    }

    Some(())
}

fn rle_rect(
    stream: &mut BitReader,
    vertical: bool,
    width: u32,
    height: u32,
    mut f: impl FnMut(u32, u32, u8),
) {
    for (i, v) in decode_rle(stream, BPP, width * height).enumerate() {
        let i = i as u32;
        let (x, y) = match vertical {
            true => (i / height, i % height),
            false => (i % width, i / width),
        };
        f(x, y, v as u8);
    }
}

fn decode_rle(
    stream: &mut BitReader,
    value_size: u8,
    mut items: u32,
) -> impl Iterator<Item = u32> + '_ {
    let mut v = 0;
    let mut run_length = 0;
    core::iter::from_fn(move || {
        if items == 0 {
            return None;
        }
        if run_length == 0 {
            run_length = stream.read_elias_delta()?;
            v = stream.read_bits(value_size)?;
        }
        items -= 1;
        run_length -= 1;
        Some(v)
    })
}

fn difflist(stream: &mut BitReader, vertical: bool, x: u32, y: u32, w: u32, h: u32) -> Option<()> {
    let mut i = stream.read_elias_delta()? - 1;
    while i < w * h {
        let (dx, dy) = match vertical {
            false => (i % w, i / w),
            true => (i / h, i % h),
        };
        set(x + dx, y + dy, stream.read_bits(BPP)? as u8);
        i += stream.read_elias_delta()?;
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

#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm::unreachable()
}
