use std::net::IpAddr;
use tmon_capture::pending::{MAX_PER_DRAIN, PendingRemoteNames};

#[test]
fn duplicates_collapse_and_latest_wins() {
    let p = PendingRemoteNames::default();
    let ip: IpAddr = "1.1.1.1".parse().unwrap();
    p.push(ip, "a.example".into());
    p.push(ip, "b.example".into());
    assert_eq!(p.len(), 1);
    let batch = p.drain();
    assert_eq!(batch, vec![(ip, "b.example".into())]);
    assert!(p.is_empty());
}

#[test]
fn buffer_is_capped_per_drain() {
    let p = PendingRemoteNames::default();
    for i in 0..(MAX_PER_DRAIN as u32 + 500) {
        p.push(IpAddr::from(i.to_be_bytes()), "x.example".into());
    }
    assert_eq!(p.len(), MAX_PER_DRAIN);
    // updating a known IP still works at the cap
    p.push(IpAddr::from(0u32.to_be_bytes()), "y.example".into());
    assert_eq!(p.drain().len(), MAX_PER_DRAIN);
}
