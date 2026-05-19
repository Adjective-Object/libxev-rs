//! Low-level FFI bindings to [libxev](https://github.com/mitchellh/libxev).
//!
//! The bindings in this crate are generated at build time by `bindgen` from
//! the vendored `include/xev.h`. The underlying static library is compiled
//! from vendored Zig sources via `zig build` invoked from `build.rs`.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::MaybeUninit;

    #[test]
    fn loop_init_and_deinit() {
        unsafe {
            let mut l = MaybeUninit::<xev_loop>::zeroed();
            let rc = xev_loop_init(l.as_mut_ptr());
            assert_eq!(rc, 0, "xev_loop_init failed");
            xev_loop_deinit(l.as_mut_ptr());
        }
    }
}
