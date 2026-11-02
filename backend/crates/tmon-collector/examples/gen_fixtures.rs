//! Regenerate backend/tests/fixtures: `cargo run -p tmon-collector --example gen_fixtures [dir]`.
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures"));
    tmon_collector::fixtures::corpus::generate(&dir)?;
    println!("fixtures written to {}", dir.display());
    Ok(())
}
