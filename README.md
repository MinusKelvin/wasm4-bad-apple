# w4-bad-apple

Bad Apple!! music video on the [WASM-4](https://wasm4.org) fantasy console.

## Building

First, you need to create a `frames` directory containing an image for each frame
of the video. It's a lot of data, so I don't include it in the repository. To
get the frames for Bad Apple!!:

```shell
mkdir frames
youtube-dl UkgK8eUdpAo
ffmpeg -i '<the file>' frames/%d.png
```

The music file for Bad Apple!! is included in the repository because it's small,
not easily available anywhere, and the music system included here is rather
specialized and probably won't work with other MIDI files. Thanks to
@analog-hors for implementing it!

Build the cart by running:

```shell
cargo --release --features use-elias-gamma
```

Then run it with:

```shell
w4 run target/wasm32-unknown-unknown/release/cart.wasm
```

You can run cargo with `+nightly -Z build-std=core -Z build-std-features=panic_immediate_abort`
and run the cart through `wasm-opt -Oz -c` to get a smaller cart, but it doesn't
affect addressable memory usage and so won't help if the video is too big.

## Customizing

Line 16 of `build/main.rs` contains a number of constants that can be used to
adjust the quality of the resulting video, including frame size, downscale
filter, framerate, and the video start frame and length. 4-color video can be
encoded by putting four color in the palette and modifying `src/lib.rs` line 17
to 2 BPP.

If you try to encode a video that is too large, you will get a linker error
saying the initial memory is too small. If this happens, you can modify the
linker arguments in `.cargo/config.toml` to increase the available memory. Note
however, that if you do this, the cart will require a modified wasm4 emulator.

If the `--features use-elias-gamma` argument is not present when building, Elias
delta coding will be used instead. This saves space with large frame sizes, but
Elias gamma coding uses less space with smaller frame sizes.
