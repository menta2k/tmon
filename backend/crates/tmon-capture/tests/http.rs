mod common;

use common::*;
use tmon_capture::fixtures::{addr, tcp_stream};
use tmon_capture::http::{self, HttpResult};
use tmon_core::naming::NameSource;

const CLIENT: &str = "192.168.88.10:40000";

#[test]
fn host_is_extracted_with_port_stripped() {
    assert_eq!(
        http::host(b"GET / HTTP/1.1\r\nHost: Example.org:8080\r\n\r\n"),
        HttpResult::Done(Some("Example.org".into()))
    );
    assert_eq!(
        http::host(b"GET / HTTP/1.1\r\nhost: [::1]:80\r\n\r\n"),
        HttpResult::Done(Some("::1".into()))
    );
    assert_eq!(
        http::host(b"GET / HTTP/1.1\r\nUser-Agent: x\r\n\r\n"),
        HttpResult::Done(None)
    );
    assert_eq!(
        http::host(b"GET / HTTP/1.1\r\nHost: a"),
        HttpResult::Partial
    );
    assert_eq!(
        http::host(b"GET / HTTP/1.1\r\n\x01\r\n\r\n"),
        HttpResult::Invalid
    );
    assert!(http::looks_like_request(b"POST /x"));
    assert!(!http::looks_like_request(b"\x16\x03\x01"));
}

#[test]
fn plain_http_request_names_the_connection() {
    let mut r = Rig::new();
    let req = b"GET /index.html HTTP/1.1\r\nHost: Neverssl.com\r\nAccept: */*\r\n\r\n";
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr("34.223.124.45:80"),
        9,
        req,
        20,
    ));
    let n = r.name(addr(CLIENT), addr("34.223.124.45:80"), 6).unwrap();
    assert_eq!((&*n.host, n.source), ("neverssl.com", NameSource::Http));
    assert_eq!(r.counters().http_named, 1);
}

#[test]
fn invalid_or_overlong_hosts_are_rejected() {
    let mut r = Rig::new();
    let long = format!("GET / HTTP/1.1\r\nHost: {}.com\r\n\r\n", "a".repeat(300));
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr("1.1.1.1:80"),
        1,
        long.as_bytes(),
        1460,
    ));
    r.feed(&tcp_stream(
        addr("192.168.88.11:1"),
        addr("1.1.1.1:80"),
        1,
        b"GET / HTTP/1.1\r\nHost: a b\r\n\r\n",
        1460,
    ));
    assert!(r.name(addr(CLIENT), addr("1.1.1.1:80"), 6).is_none());
    assert_eq!(r.counters().name_invalid, 2);
}
