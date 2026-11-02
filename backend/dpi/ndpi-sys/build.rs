//! Locate libndpi with pkg-config (honouring `NDPI_PREFIX`, as set by
//! `just dpi-build`) and generate bindings for the `ndpi_*` API only.

use std::env;
use std::path::PathBuf;

/// Include directories for bindgen: `NDPI_INCLUDE_DIR` (headers only, e.g.
/// for type-checking without an installed library), else pkg-config.
fn include_dirs() -> Vec<PathBuf> {
    if let Ok(dir) = env::var("NDPI_INCLUDE_DIR") {
        println!("cargo:rustc-link-lib=ndpi");
        return vec![PathBuf::from(dir)];
    }
    probe()
}

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=shim.c");
    println!("cargo:rerun-if-env-changed=NDPI_PREFIX");
    println!("cargo:rerun-if-env-changed=NDPI_INCLUDE_DIR");
    let includes = include_dirs();
    let mut b = bindgen::Builder::default()
        .header("wrapper.h")
        .allowlist_function("ndpi_.*")
        .allowlist_type("ndpi_.*")
        .allowlist_var("NDPI_.*")
        .allowlist_function("tmon_ndpi_.*")
        // bindgen mis-sizes these packed-bitfield structs; Rust only handles
        // pointers to them and reads fields through shim.c.
        .opaque_type("ndpi_flow_struct")
        .opaque_type("ndpi_flow_tcp_struct")
        .opaque_type("ndpi_flow_udp_struct")
        .default_enum_style(bindgen::EnumVariation::Consts)
        .prepend_enum_name(false)
        .layout_tests(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for dir in &includes {
        b = b.clang_arg(format!("-I{}", dir.display()));
    }
    let mut cc = cc::Build::new();
    cc.file("shim.c").warnings(true);
    for dir in &includes {
        cc.include(dir);
    }
    cc.compile("tmon_ndpi_shim");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    b.generate()
        .expect("bindgen failed on ndpi_api.h")
        .write_to_file(out.join("bindings.rs"))
        .expect("write bindings");
}

/// pkg-config `libndpi >= 6.0`, honouring `NDPI_PREFIX`.
fn probe() -> Vec<PathBuf> {
    if let Ok(prefix) = env::var("NDPI_PREFIX") {
        let pc = PathBuf::from(prefix).join("lib/pkgconfig");
        let path = match env::var("PKG_CONFIG_PATH") {
            Ok(old) if !old.is_empty() => format!("{}:{old}", pc.display()),
            _ => pc.display().to_string(),
        };
        // SAFETY: build scripts are single-threaded; nothing reads the
        // environment concurrently.
        unsafe { env::set_var("PKG_CONFIG_PATH", path) };
    }
    let lib = pkg_config::Config::new()
        .atleast_version("6.0")
        .probe("libndpi")
        .expect("libndpi >= 6.0 not found: run `just dpi-build` or set NDPI_PREFIX");
    // Exposed to dependents as DEP_NDPI_LIBDIR (tmon-dpi sets an rpath, as
    // tmon starts the sidecar with a clean environment).
    if let Some(dir) = lib.link_paths.first() {
        println!("cargo:libdir={}", dir.display());
    }
    lib.include_paths
}
