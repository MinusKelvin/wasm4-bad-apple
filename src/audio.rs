// Author: analog-hors

use crate::bitstream::BitStream;

const PULSE_1: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pulse_one.bin"));
const PULSE_2: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pulse_two.bin"));
const TRIANGLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/triangle.bin"));
const NOISE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/noise.bin"));

pub struct Program {
    pulse_one: ChannelPlayer,
    pulse_two: ChannelPlayer,
    triangle: ChannelPlayer,
    noise: ChannelPlayer,
}

impl Program {
    pub fn new() -> Self {
        Self {
            pulse_one: ChannelPlayer::new(
                ChannelReader {
                    stream: BitStream::new(PULSE_1),
                    delta_bits: 3,
                    deltas: &[13, 14, 26, 27, 890, 1676],
                    length_bits: 2,
                    lengths: &[9, 22, 35],
                    pitch_bits: 5,
                    pitches: &[277, 294, 311, 330, 349, 370, 392, 415, 440, 466, 494, 554, 587, 622, 659, 698, 740, 784],
                },
                Channel::PulseOne,
                30,
            ),
            pulse_two: ChannelPlayer::new(
                ChannelReader {
                    stream: BitStream::new(PULSE_2),
                    delta_bits: 3,
                    deltas: &[0, 6, 7, 13, 14, 26, 27, 2541],
                    length_bits: 0,
                    lengths: &[2],
                    pitch_bits: 0,
                    pitches: &[165],
                },
                Channel::PulseTwo,
                30,
            ),
            triangle: ChannelPlayer::new(
                ChannelReader {
                    stream: BitStream::new(TRIANGLE),
                    delta_bits: 4,
                    deltas: &[6, 7, 13, 14, 19, 20, 26, 104, 105, 158, 209, 210, 837],
                    length_bits: 3,
                    lengths: &[3, 9, 16, 22, 101, 206],
                    pitch_bits: 5,
                    pitches: &[31, 33, 35, 37, 39, 41, 46, 52, 55, 62, 65, 69, 73, 78, 92, 98, 104],
                },
                Channel::Triangle,
                100,
            ),
            noise: ChannelPlayer::new(
                ChannelReader {
                    stream: BitStream::new(NOISE),
                    delta_bits: 4,
                    deltas: &[12, 26, 27, 40, 41, 52, 53, 786, 841, 863],
                    length_bits: 1,
                    lengths: &[3, 9],
                    pitch_bits: 0,
                    pitches: &[698],
                },
                Channel::Noise,
                30,
            )
        }
    }

    pub fn update(&mut self) {
        self.pulse_one.tick();
        self.pulse_two.tick();
        self.triangle.tick();
        self.noise.tick();
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Channel {
    PulseOne,
    PulseTwo,
    Triangle,
    Noise,
}

#[derive(Debug, Clone, Copy)]
pub struct Tone {
    pub start_freq: u16,
    pub end_freq: u16,
    pub attack: u16,
    pub decay: u16,
    pub sustain: u16,
    pub release: u16,
    pub peak: u8,
    pub volume: u8,
    pub channel: Channel,
}

fn tone(t: Tone) {
    let frequency = (t.end_freq as u32) << 16 | t.start_freq as u32;
    let mut duration = t.attack as u32;
    duration = (duration << 16) | t.decay as u32;
    duration = (duration << 16) | t.sustain as u32;
    duration = (duration << 16) | t.release as u32;
    crate::wasm4::tone(
        frequency,
        duration,
        (t.peak as u32) << 8 | t.volume as u32,
        t.channel as u32,
    );
}

struct Note {
    delta: u32,
    length: u16,
    pitch: u16,
}

struct ChannelReader {
    stream: BitStream<'static>,
    delta_bits: u8,
    deltas: &'static [u32],
    length_bits: u8,
    lengths: &'static [u16],
    pitch_bits: u8,
    pitches: &'static [u16],
}

impl ChannelReader {
    fn next(&mut self) -> Option<Note> {
        let delta = self.deltas[self.stream.read_bits(self.delta_bits)? as usize];
        let length = self.lengths[self.stream.read_bits(self.length_bits)? as usize];
        let pitch = self.pitches[self.stream.read_bits(self.pitch_bits)? as usize];
        Some(Note {
            delta,
            length,
            pitch,
        })
    }
}

struct ChannelPlayer {
    reader: ChannelReader,
    channel: Channel,
    volume: u8,
    note: Option<Note>,
}

impl ChannelPlayer {
    fn new(mut reader: ChannelReader, channel: Channel, volume: u8) -> Self {
        let note = reader.next();
        Self {
            reader,
            channel,
            note,
            volume,
        }
    }

    fn tick(&mut self) {
        if let Some(note) = &mut self.note {
            if note.delta != 0 {
                note.delta -= 1;
            }
            if note.delta == 0 {
                let mut t = Tone {
                    start_freq: note.pitch,
                    end_freq: note.pitch,
                    attack: 0,
                    decay: 0,
                    sustain: note.length,
                    release: 0,
                    channel: self.channel,
                    peak: self.volume,
                    volume: self.volume,
                };
                if let Channel::Noise = t.channel {
                    t.release = note.length;
                    t.decay = 1;
                    t.sustain = 0;
                    t.peak = 100;
                    t.volume = 5;
                    t.end_freq = 1000;
                }
                tone(t);
                self.note = self.reader.next();
            }
        }
    }
}
