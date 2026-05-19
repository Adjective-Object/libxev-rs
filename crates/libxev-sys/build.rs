use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("vendor").join("libxev");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Re-run when vendored sources change.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    rerun_if_tree_changed(&vendor_dir.join("src"));
    rerun_if_tree_changed(&vendor_dir.join("include"));
    println!(
        "cargo:rerun-if-changed={}",
        vendor_dir.join("build.zig").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        vendor_dir.join("build.zig.zon").display()
    );
    println!("cargo:rerun-if-env-changed=ZIG");

    // ---- Build libxev with Zig ----
    let zig = env::var("ZIG").unwrap_or_else(|_| "zig".to_string());

    let optimize = if cfg!(feature = "debug") {
        "Debug"
    } else {
        "ReleaseFast"
    };

    let zig_cache = out_dir.join("zig-cache");
    let zig_out = out_dir.join("zig-out");
    std::fs::create_dir_all(&zig_cache).expect("create zig cache dir");
    std::fs::create_dir_all(&zig_out).expect("create zig out dir");

    let target = env::var("TARGET").unwrap();
    let zig_target = rust_target_to_zig(&target);

    let mut cmd = Command::new(&zig);
    cmd.current_dir(&vendor_dir)
        .arg("build")
        .arg(format!("-Doptimize={optimize}"))
        .arg("-Demit-man-pages=false")
        .arg("--cache-dir")
        .arg(&zig_cache)
        .arg("--prefix")
        .arg(&zig_out);
    if let Some(zt) = zig_target {
        cmd.arg(format!("-Dtarget={zt}"));
    }

    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke `{zig} build`: {e}"));
    if !status.success() {
        panic!("`zig build` failed with status {status}");
    }

    // The static library lands at zig-out/lib/libxev.a on Unix; on Windows
    // it is xev.lib. The Zig artifact name is "xev", so Cargo's link name is
    // "xev" (i.e. -lxev).
    let lib_dir = zig_out.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=xev");

    // libxev links libc; on Windows it also needs ws2_32 and mswsock.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "windows" => {
            println!("cargo:rustc-link-lib=dylib=ws2_32");
            println!("cargo:rustc-link-lib=dylib=mswsock");
        }
        "macos" | "ios" => {
            // libSystem provides libc; nothing extra typically required.
        }
        _ => {
            // Linux/BSD: libxev uses pthreads for the threadpool.
            println!("cargo:rustc-link-lib=dylib=pthread");
        }
    }

    // Expose include dir / lib dir to dependents.
    let include_dir = zig_out.join("include");
    println!("cargo:include={}", include_dir.display());
    println!("cargo:lib={}", lib_dir.display());

    // ---- Generate Rust bindings ----
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_dir.display()))
        // Force the `long double` branch of XEV_ALIGN_T (16 bytes) so the
        // 24-byte THREADPOOL_{BATCH,TASK} arrays in xev.h don't underflow
        // against glibc's 32-byte max_align_t.
        .clang_arg("-std=c99")
        .allowlist_function("xev_.*")
        .allowlist_type("xev_.*")
        .allowlist_var("XEV_.*")
        .derive_default(true)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("failed to generate bindings for xev.h");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");
}

fn rerun_if_tree_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            rerun_if_tree_changed(&p);
        } else {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }
}

/// Translate a Rust target triple into a Zig target triple. Returns `None`
/// when we should let Zig pick its native default (host build).
fn rust_target_to_zig(rust_target: &str) -> Option<String> {
    let host = env::var("HOST").unwrap_or_default();
    if rust_target == host {
        return None;
    }

    // A minimal translation that covers the common tier-1 targets. Add more as
    // the wrapper grows.
    let mut parts = rust_target.split('-');
    let arch = parts.next()?;
    let _vendor = parts.next();
    let os = parts.next()?;
    let abi = parts.next();

    let zig_arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "i686" => "x86",
        "arm" => "arm",
        "riscv64gc" => "riscv64",
        other => other,
    };

    let zig_os = match os {
        "linux" => "linux",
        "darwin" => "macos",
        "windows" => "windows",
        "freebsd" => "freebsd",
        other => other,
    };

    let zig_abi = abi.map(|a| match a {
        "gnu" => "gnu",
        "musl" => "musl",
        "msvc" => "msvc",
        other => other,
    });

    Some(match zig_abi {
        Some(abi) => format!("{zig_arch}-{zig_os}-{abi}"),
        None => format!("{zig_arch}-{zig_os}"),
    })
}
