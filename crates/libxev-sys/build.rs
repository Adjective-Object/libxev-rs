use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let extended_api = env::var_os("CARGO_FEATURE_EXTENDED_API").is_some();
    let local_fork = env::var_os("CARGO_FEATURE_LOCAL_FORK").is_some();

    // `local-fork` mutates the committed `vendor/libxev-fork` tree and is
    // intended strictly as a developer workflow. Refuse to use it in release
    // builds so a downstream consumer doesn't accidentally re-vendor the
    // fork from whatever happens to be on their machine.
    if local_fork && env::var("PROFILE").as_deref() == Ok("release") {
        panic!(
            "feature `local-fork` is for development only and cannot be used \
             in release builds. If you want to consume the vendored fork, \
             enable `--features extended-api` instead."
        );
    }

    let fork_dir = manifest_dir.join("vendor").join("libxev-fork");
    let upstream_dir = manifest_dir.join("vendor").join("libxev");

    // `local-fork` re-vendors `vendor/libxev-fork` from `$LIBXEV_SOURCE`
    // before building. This is a developer workflow — it mutates the
    // committed source tree on purpose.
    println!("cargo:rerun-if-env-changed=LIBXEV_SOURCE");
    if local_fork {
        let src = env::var("LIBXEV_SOURCE").unwrap_or_else(|_| {
            panic!(
                "feature `local-fork` is enabled but LIBXEV_SOURCE is not set; \
                 point it at a local libxev checkout to re-vendor it"
            )
        });
        let src_path = PathBuf::from(&src);
        if !src_path.join("build.zig").is_file() {
            panic!(
                "LIBXEV_SOURCE={src} does not look like a libxev checkout \
                 (no build.zig)"
            );
        }
        re_vendor_fork(&src_path, &fork_dir);
        rerun_if_tree_changed(&src_path.join("src"));
        rerun_if_tree_changed(&src_path.join("include"));
        println!(
            "cargo:rerun-if-changed={}",
            src_path.join("build.zig").display()
        );
    }

    let vendor_dir = if extended_api {
        if !fork_dir.join("build.zig").is_file() {
            panic!(
                "feature `extended-api` is enabled but {} is empty; \
                 build once with `--features local-fork` and \
                 LIBXEV_SOURCE pointing at the fork to populate it",
                fork_dir.display()
            );
        }
        fork_dir
    } else {
        upstream_dir
    };

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
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let zig_target = rust_target_to_zig(&target);

    // On Windows we need to teach the Zig static library to bundle
    // compiler_rt symbols (`__chkstk_ms` et al.) which are otherwise
    // unresolved when MSVC's linker pulls in xev.lib. We don't want to
    // mutate the vendored sources, so we copy them into OUT_DIR and
    // hot-patch `build.zig` there.
    let build_dir = if target_os == "windows" {
        let staged = out_dir.join("build-tree");
        prepare_windows_build_tree(&vendor_dir, &staged);
        staged
    } else {
        vendor_dir.clone()
    };

    let mut cmd = Command::new(&zig);
    cmd.current_dir(&build_dir)
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
    match target_os.as_str() {
        "windows" => {
            println!("cargo:rustc-link-lib=dylib=ws2_32");
            println!("cargo:rustc-link-lib=dylib=mswsock");
            if target_env == "msvc" {
                // Zig's std references `LdrRegisterDllNotification`, which
                // ntdll.dll exports but the Windows SDK's ntdll.lib does
                // not. Generate a tiny import lib from a .def using
                // lib.exe and link against it.
                generate_ntdll_extras_lib(&target, &out_dir);
                println!("cargo:rustc-link-search=native={}", out_dir.display());
                println!("cargo:rustc-link-lib=dylib=ntdll_extras");
            }
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
        // Emit `unsafe extern "C"` blocks (required under edition 2024).
        .rust_edition(bindgen::RustEdition::Edition2024)
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

/// Copy a libxev checkout at `src` into `dst`, replacing any existing
/// contents. Skips VCS and build-artifact directories.
fn re_vendor_fork(src: &Path, dst: &Path) {
    const SKIP: &[&str] = &[
        ".git",
        ".github",
        ".zig-cache",
        "zig-cache",
        "zig-out",
        "website",
        "target",
        "node_modules",
    ];

    if dst.exists() {
        std::fs::remove_dir_all(dst)
            .unwrap_or_else(|e| panic!("failed to remove {}: {e}", dst.display()));
    }
    std::fs::create_dir_all(dst)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", dst.display()));

    copy_tree(src, dst, SKIP);
}

fn copy_tree(src: &Path, dst: &Path, skip: &[&str]) {
    let entries =
        std::fs::read_dir(src).unwrap_or_else(|e| panic!("failed to read {}: {e}", src.display()));
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip.iter().any(|s| *s == name_str) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let file_type = entry
            .file_type()
            .unwrap_or_else(|e| panic!("file_type {}: {e}", from.display()));
        if file_type.is_dir() {
            std::fs::create_dir_all(&to).unwrap_or_else(|e| panic!("mkdir {}: {e}", to.display()));
            copy_tree(&from, &to, skip);
        } else if file_type.is_symlink() {
            // Resolve through the link to keep the fork self-contained.
            let target = std::fs::read_link(&from)
                .unwrap_or_else(|e| panic!("readlink {}: {e}", from.display()));
            let resolved = if target.is_absolute() {
                target
            } else {
                from.parent().unwrap().join(target)
            };
            if resolved.is_dir() {
                std::fs::create_dir_all(&to).ok();
                copy_tree(&resolved, &to, skip);
            } else {
                std::fs::copy(&resolved, &to).unwrap_or_else(|e| {
                    panic!("copy {} -> {}: {e}", resolved.display(), to.display())
                });
            }
        } else {
            std::fs::copy(&from, &to)
                .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", from.display(), to.display()));
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

/// Stage a copy of the vendored libxev tree under `dst` and hot-patch
/// `build.zig` so the Windows static library bundles compiler_rt symbols
/// (`__chkstk_ms` etc.). Returning the path to the patched tree lets the
/// caller invoke `zig build` against it without mutating the committed
/// vendor directory.
fn prepare_windows_build_tree(src: &Path, dst: &Path) {
    re_vendor_fork(src, dst);

    let build_zig = dst.join("build.zig");
    let original = std::fs::read_to_string(&build_zig)
        .unwrap_or_else(|e| panic!("read {}: {e}", build_zig.display()));

    // Insert `static_lib.bundle_compiler_rt = true;` at the top of the
    // `if (target.result.os.tag == .windows)` block that configures the
    // static library. The marker must match the upstream formatting.
    let needle = "if (target.result.os.tag == .windows) {\n            static_lib.root_module.linkSystemLibrary(\"ws2_32\", .{});";
    let replacement = "if (target.result.os.tag == .windows) {\n            static_lib.bundle_compiler_rt = true;\n            static_lib.root_module.linkSystemLibrary(\"ws2_32\", .{});";

    if !original.contains(needle) {
        panic!(
            "could not locate the windows static-lib block in {} to hot-patch \
             (libxev's build.zig changed format?)",
            build_zig.display()
        );
    }

    let patched = original.replace(needle, replacement);
    std::fs::write(&build_zig, patched)
        .unwrap_or_else(|e| panic!("write {}: {e}", build_zig.display()));
}

/// Generate `ntdll_extras.lib` in `out_dir` containing import stubs for
/// ntdll symbols that the Windows SDK's `ntdll.lib` omits but Zig's std
/// references (currently just `LdrRegisterDllNotification`).
#[cfg(windows)]
fn generate_ntdll_extras_lib(target: &str, out_dir: &Path) {
    use std::io::Write;

    let def_path = out_dir.join("ntdll_extras.def");
    let lib_path = out_dir.join("ntdll_extras.lib");

    let mut def = std::fs::File::create(&def_path)
        .unwrap_or_else(|e| panic!("create {}: {e}", def_path.display()));
    writeln!(def, "LIBRARY ntdll").unwrap();
    writeln!(def, "EXPORTS").unwrap();
    writeln!(def, "    LdrRegisterDllNotification").unwrap();
    writeln!(def, "    LdrUnregisterDllNotification").unwrap();
    drop(def);

    let machine = if target.starts_with("x86_64") {
        "X64"
    } else if target.starts_with("aarch64") {
        "ARM64"
    } else if target.starts_with("i686") || target.starts_with("i586") {
        "X86"
    } else {
        panic!("unsupported windows target for ntdll_extras: {target}");
    };

    let mut lib_cmd = cc::windows_registry::find_tool(target, "lib.exe")
        .unwrap_or_else(|| panic!("could not locate lib.exe for target {target}"))
        .to_command();
    lib_cmd
        .arg(format!("/def:{}", def_path.display()))
        .arg(format!("/out:{}", lib_path.display()))
        .arg(format!("/machine:{machine}"))
        .arg("/nologo");

    let status = lib_cmd
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke lib.exe: {e}"));
    if !status.success() {
        panic!("lib.exe failed with status {status} while building ntdll_extras.lib");
    }
}

/// Stub for non-Windows hosts (the .lib generation is host-tooling driven).
#[cfg(not(windows))]
fn generate_ntdll_extras_lib(_target: &str, _out_dir: &Path) {
    panic!(
        "ntdll_extras.lib generation requires running build.rs on a Windows host; \
         cross-compiling libxev-sys to windows-msvc from another OS is unsupported"
    );
}
