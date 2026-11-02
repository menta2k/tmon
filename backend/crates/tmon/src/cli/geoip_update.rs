//! `tmon geoip-update`: download DB-IP Lite country + ASN databases (CC BY 4.0).

use anyhow::{Context, bail};
use std::io::Read;
use std::path::Path;
use time::OffsetDateTime;

use crate::config::Config;

pub const BASE_URL: &str = "https://download.db-ip.com/free";
pub const ATTRIBUTION: &str =
    "IP geolocation by DB-IP (https://db-ip.com), licensed under CC BY 4.0.";
/// Upper bound for a decompressed database (sanity check against bad downloads).
const MAX_DB_BYTES: u64 = 512 * 1024 * 1024;

/// `YYYY-MM` for the given month and the month before.
pub fn months(now: OffsetDateTime) -> [String; 2] {
    let (y, m) = (now.year(), u8::from(now.month()));
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    [format!("{y:04}-{m:02}"), format!("{py:04}-{pm:02}")]
}

pub fn url(kind: &str, month: &str) -> String {
    format!("{BASE_URL}/dbip-{kind}-lite-{month}.mmdb.gz")
}

/// Gunzip with a size cap.
pub fn gunzip(gz: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .take(MAX_DB_BYTES + 1)
        .read_to_end(&mut out)
        .context("decompressing database")?;
    if out.len() as u64 > MAX_DB_BYTES {
        bail!("database larger than {MAX_DB_BYTES} bytes");
    }
    Ok(out)
}

/// Write via a temp file in the same directory, validate it opens, then rename atomically.
pub fn install(bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    let dir = dest
        .parent()
        .context("destination has no parent directory")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dest.with_extension("mmdb.tmp");
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = maxminddb::Reader::open_readfile(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        bail!("downloaded file is not a valid mmdb: {e}");
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

async fn fetch(
    client: &reqwest::Client,
    kind: &str,
    now: OffsetDateTime,
) -> anyhow::Result<Vec<u8>> {
    for month in months(now) {
        let u = url(kind, &month);
        let res = client
            .get(&u)
            .send()
            .await
            .with_context(|| format!("GET {u}"))?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        let res = res.error_for_status().with_context(|| format!("GET {u}"))?;
        println!("downloaded {u}");
        return gunzip(&res.bytes().await?);
    }
    bail!("no {kind} database published for the current or previous month")
}

pub async fn run(config: Option<&Path>) -> anyhow::Result<()> {
    let cfg = Config::load(config)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("tmon/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let now = OffsetDateTime::now_utc();
    for (kind, dest) in [
        ("country", &cfg.geoip.country_db),
        ("asn", &cfg.geoip.asn_db),
    ] {
        let db = fetch(&client, kind, now).await?;
        install(&db, dest).with_context(|| format!("installing {}", dest.display()))?;
        println!("installed {}", dest.display());
    }
    println!("{ATTRIBUTION}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use time::macros::datetime;

    #[test]
    fn month_fallback() {
        assert_eq!(
            months(datetime!(2026-10-31 0:00 UTC)),
            ["2026-10", "2026-09"]
        );
        assert_eq!(
            months(datetime!(2027-01-02 0:00 UTC)),
            ["2027-01", "2026-12"]
        );
        assert_eq!(
            url("asn", "2026-10"),
            "https://download.db-ip.com/free/dbip-asn-lite-2026-10.mmdb.gz"
        );
    }

    #[test]
    fn install_validates_and_renames() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("geo/c.mmdb");
        assert!(install(b"not a db", &dest).is_err());
        assert!(!dest.exists() && !dest.with_extension("mmdb.tmp").exists());
        let good = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/geo/GeoIP2-Country-Test.mmdb"
        ))
        .unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&good).unwrap();
        install(&gunzip(&gz.finish().unwrap()).unwrap(), &dest).unwrap();
        assert!(dest.exists());
    }
}
