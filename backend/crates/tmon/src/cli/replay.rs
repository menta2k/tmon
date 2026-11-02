//! `tmon replay <file.pcap> [--to addr] [--speed 1x|Nx|max]`.

use std::path::Path;
use std::time::Duration;
use tmon_collector::pcap::read_udp;
use tokio::net::UdpSocket;

/// Parse `--speed`: `max` → None (no pacing), `Nx` → Some(N).
pub fn parse_speed(s: &str) -> anyhow::Result<Option<f64>> {
    if s == "max" {
        return Ok(None);
    }
    let n: f64 = s
        .trim_end_matches('x')
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --speed `{s}`"))?;
    if n <= 0.0 {
        anyhow::bail!("--speed must be positive");
    }
    Ok(Some(n))
}

pub async fn run(file: &Path, to: &str, speed: &str) -> anyhow::Result<()> {
    let speed = parse_speed(speed)?;
    let target: std::net::SocketAddr = to
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --to `{to}`"))?;
    let packets = read_udp(std::fs::File::open(file)?)?;
    let bind = if target.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let sock = UdpSocket::bind(bind).await?;
    let mut prev: Option<i64> = None;
    for p in &packets {
        if let (Some(factor), Some(last)) = (speed, prev) {
            let gap_us = ((p.ts_us - last).max(0) as f64 / factor) as u64;
            tokio::time::sleep(Duration::from_micros(gap_us)).await;
        }
        prev = Some(p.ts_us);
        sock.send_to(&p.payload, target).await?;
    }
    println!("replayed {} datagrams to {target}", packets.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds() {
        assert_eq!(parse_speed("max").unwrap(), None);
        assert_eq!(parse_speed("1x").unwrap(), Some(1.0));
        assert_eq!(parse_speed("10x").unwrap(), Some(10.0));
        assert!(parse_speed("0x").is_err());
        assert!(parse_speed("fast").is_err());
    }
}
