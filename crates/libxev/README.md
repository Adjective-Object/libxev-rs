# libxev

This crate provides rust bindings to [libxev](https://github.com/mitchellh/libxev).
It has no affiliation with the original project, and is provided as a convenience
to others in the rust ecosystem.

If you are mitchellh and want to take this crate name for an official rust crate,
please reach out over on github!

## Build Requirements

This compiles libxev with `zig`, so you need to [install zig](https://ziglang.org/download/#release-0.16.0).
Additionally, you will need `libclang` for running bindgen in the build script

## Extended API

This crate can optionally build a fork of libxev with exposes more of the
functionality to the c-api surface. To use this, enable the `extended-api`
crate feature.

During development, you can enable `local-fork` and `extended-api` to build out
of `env.LIBXEV_SOURCE`, pointed to your local checkout

```sh
cargo add libxev                         # build from unmodified libxev
cargo add libxev --features extended-api # build our fork
```
