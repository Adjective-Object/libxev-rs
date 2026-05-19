# libxev-rs

rust bindings to libxev

- `libxev-sys`: raw c-api bindings
- `libxev`: idiomatic rust bindings

## Extended API

This crate can optionally build a fork of libxev with exposes more of the
functionality to the c-api surface. To use this, enable `extended-api`.

During development, you can enable `local-fork` and `extended-api` to build out
of `env.LIBXEV_SOURCE`, pointed to your local checkout

```sh
cargo add libxev                         # build from unmodified libxev
cargo add libxev --features extended-api # build our fork
```

## Development

### Setup

1. Install [zig](https://ziglang.org/download/#release-0.16.0) and [cargo](https://rustup.rs/)

```sh
wget https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz
tar xf zig-x86_64-linux-0.16.0.tar.xz -C $HOME
echo 'export PATH="$PATH:$HOME/zig-x86_64-linux-0.16.0"' >> ~/.profile
```

Or on windows, use `scoop`, because [winget takes upwards of 20 minutes](https://github.com/microsoft/winget-cli/issues/3306).
to extract the files

This may still be slow due to interference from Windows Defender. You may want
to add an exeption for `~/scoop`.

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression
scoop checkup # then fix issues reported by scoop checkup

scoop install zig
scoop install llvm # for libclang bindings, used in build.rs in bindgen
```

2. Install binstall -> nextest

```sh
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
cargo binstall cargo-nextest
```

### Local Build Scenarios

```sh
cargo nextest --no-fail-fast # Run tests

# iterate on your local fork of libxev
# e.g. https://github.com/Adjective-Object/libxev/tree/file-api
LIBXEV_SOURCE=/path/to/libxev-fork cargo build --features local-fork
```
