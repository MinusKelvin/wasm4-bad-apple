#!/usr/bin/python3

# Author: analog-hors

import pretty_midi
from os import environ

class Bitstream:
    buffer: list
    shift: int

    def __init__(self):
        self.buffer = [0]
        self.shift = 0

    def write(self, value: int, bits: int):
        for i in range(bits):
            self.write_bool(bool((value >> i) & 1))

    def write_bool(self, value: bool):
        self.buffer[-1] |= int(value) << self.shift
        self.shift += 1
        if self.shift == 8:
            self.buffer.append(0)
            self.shift = 0

fps = 65.5
channels = ["noise", "triangle", "pulse"]
midi_data = pretty_midi.PrettyMIDI("music.mid")
for instrument, name in zip(midi_data.instruments, channels):
    deltas = set()
    lengths = set()
    pitches = set()
    notes = []
    prev_time = 0
    for note in instrument.notes:
        time = round(note.start * fps)
        delta = time - prev_time
        length = round((note.end - note.start) * fps)
        pitch = round(pretty_midi.note_number_to_hz(note.pitch))
        deltas.add(delta)
        lengths.add(length)
        pitches.add(pitch)
        notes.append((delta, length, pitch))
        prev_time = time
    deltas = sorted(deltas)
    lengths = sorted(lengths)
    pitches = sorted(pitches)
    print(name)
    print(deltas)
    print(lengths)
    print(pitches)
    stream = Bitstream()
    for delta, length, pitch in notes:
        delta = deltas.index(delta)
        length = lengths.index(length)
        pitch = pitches.index(pitch)
        if name == "noise":
            stream.write(delta, 3)
        if name == "triangle":
            stream.write(delta, 4)
            stream.write(length, 3)
            stream.write(pitch, 5)
        if name == "pulse":
            stream.write(delta, 3)
            stream.write(length, 2)
            stream.write(pitch, 5)

    with open(f"{environ['OUT_DIR']}/{name}.bin", "wb+") as file:
        file.write(bytes(stream.buffer))
