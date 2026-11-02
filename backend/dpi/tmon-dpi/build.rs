//! With nDPI from a non-system prefix (`just dpi-build`), embed its library
//! directory as an rpath: tmon starts the sidecar with a clean environment,
//! so `LD_LIBRARY_PATH` is not available. System installs need nothing.

fn main() {
    println!("cargo:rerun-if-env-changed=DEP_NDPI_LIBDIR");
    if std::env::var_os("CARGO_FEATURE_NDPI").is_none() {
        return;
    }
    if let Ok(dir) = std::env::var("DEP_NDPI_LIBDIR") {
        let system = ["/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/local/lib"];
        if !system.iter().any(|s| dir.trim_end_matches('/') == *s) {
            println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{dir}");
            println!("cargo:rustc-link-arg-tests=-Wl,-rpath,{dir}");
        }
    }
}
