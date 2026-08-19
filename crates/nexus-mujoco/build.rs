//! Compile the C shim against the real MuJoCo headers, into its own dylib.
//!
//! The shim is the only code that knows mjModel/mjvScene struct layouts, and
//! it learns them from the headers the wheel ships — so it can never drift
//! from the library it talks to. It links nothing: it is built with
//! `-undefined dynamic_lookup` (macOS) so its `mj_*` references resolve at
//! runtime, after lib.rs has dlopen'd libmujoco into the process. No MuJoCo
//! on the machine → no headers → the crate builds anyway and reports
//! unavailable at runtime, the same stance nexus-engine takes.

use std::path::PathBuf;
use std::process::Command;

/// Headers must be the ONES SHIPPED WITH THE DYLIB — struct offsets are the
/// contract. A neighboring source checkout of a different MuJoCo version
/// compiles fine and then reads garbage through every array pointer, so the
/// include dir is derived from the dylib's own directory and never probed
/// independently.
fn include_beside(dylib: &std::path::Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NEXUS_MUJOCO_INCLUDE") {
        let p = PathBuf::from(p);
        if p.join("mujoco/mujoco.h").is_file() {
            return Some(p);
        }
    }
    let inc = dylib.parent()?.join("include");
    inc.join("mujoco/mujoco.h").is_file().then_some(inc)
}

fn find_dylib() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NEXUS_MUJOCO_LIB") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let lib = root.join("mujoco-stack/venv/lib");
    if let Ok(rd) = std::fs::read_dir(&lib) {
        for e in rd.flatten() {
            let pkg = e.path().join("site-packages/mujoco");
            if let Ok(files) = std::fs::read_dir(&pkg) {
                for f in files.flatten() {
                    let name = f.file_name().to_string_lossy().to_string();
                    if name.starts_with("libmujoco.") && name.ends_with(".dylib") {
                        return Some(f.path());
                    }
                    if name.starts_with("libmujoco.so") {
                        return Some(f.path());
                    }
                }
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-env-changed=NEXUS_MUJOCO_INCLUDE");
    println!("cargo:rerun-if-env-changed=NEXUS_MUJOCO_LIB");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let stub = || {
        // Stub build: runtime reports why_unavailable.
        println!("cargo:rustc-env=NEXUS_MJSHIM=");
        println!("cargo:rustc-env=NEXUS_MUJOCO_DYLIB=");
    };
    let Some(dylib) = find_dylib() else {
        stub();
        return;
    };
    let Some(inc) = include_beside(&dylib) else {
        stub();
        return;
    };

    let shim = out.join(if cfg!(target_os = "macos") { "libnxmjshim.dylib" } else { "libnxmjshim.so" });
    let mut c = Command::new(std::env::var("CC").unwrap_or_else(|_| "cc".into()));
    c.arg("-O2").arg("-fPIC").arg("-I").arg(&inc).arg("src/shim.c").arg("-o").arg(&shim);
    if cfg!(target_os = "macos") {
        c.arg("-dynamiclib").arg("-undefined").arg("dynamic_lookup");
    } else {
        c.arg("-shared");
    }
    let st = c.status().expect("running the C compiler");
    if !st.success() {
        panic!("shim compile failed against {}", inc.display());
    }
    println!("cargo:rustc-env=NEXUS_MJSHIM={}", shim.display());
    println!("cargo:rustc-env=NEXUS_MUJOCO_DYLIB={}", dylib.canonicalize().unwrap().display());
}
