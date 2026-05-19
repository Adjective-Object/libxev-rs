# libxev-sys

Low-level FFI bindings to [libxev](https://github.com/mitchellh/libxev).

The libxev source is vendored under [`vendor/libxev`](vendor/libxev). The
`build.rs` script:

1. Invokes `zig build -Doptimize=ReleaseFast` inside the vendored tree,
   writing artifacts into Cargo's `OUT_DIR`.
2. Links the resulting static `libxev.a` (`-lxev`) into the crate.
3. Runs `bindgen` against `vendor/libxev/include/xev.h` and writes
   `bindings.rs` into `OUT_DIR`, which `src/lib.rs` `include!`s.

## Requirements

The build script shells out to a `zig` binary, so you need a Zig toolchain on
`PATH` (override with the `ZIG` environment variable). The vendored libxev
sources currently require Zig **≥ 0.16.0**. `bindgen` additionally needs
`libclang` available on your system (e.g. `apt install libclang-dev`).

## Features

- `debug` — compile libxev with Zig's `Debug` optimization instead of the
  default `ReleaseFast`.

## Generated symbols

All `xev_*` functions/types and `XEV_*` constants from `include/xev.h` are
re-exported at the crate root. Higher-level Rust APIs live in the
[`libxev`](../libxev) crate.
