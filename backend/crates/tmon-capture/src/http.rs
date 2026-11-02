//! Plain HTTP request → `Host` (R7, FR-007).

const METHODS: [&[u8]; 9] = [
    b"GET ",
    b"POST ",
    b"HEAD ",
    b"PUT ",
    b"DELETE ",
    b"OPTIONS ",
    b"PATCH ",
    b"CONNECT ",
    b"TRACE ",
];
const MAX_HEADERS: usize = 64;

/// Payload starts with an HTTP method token.
pub fn looks_like_request(p: &[u8]) -> bool {
    METHODS.iter().any(|m| p.starts_with(m))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpResult {
    /// Need more bytes.
    Partial,
    /// Complete request head; host header value with the port stripped
    /// (not yet normalized), if any.
    Done(Option<String>),
    Invalid,
}

/// Parse the request head and extract `Host`.
pub fn host(p: &[u8]) -> HttpResult {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(p) {
        Ok(httparse::Status::Partial) => HttpResult::Partial,
        Ok(httparse::Status::Complete(_)) => HttpResult::Done(
            req.headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("host"))
                .and_then(|h| std::str::from_utf8(h.value).ok())
                .map(strip_port),
        ),
        Err(_) => HttpResult::Invalid,
    }
}

fn strip_port(v: &str) -> String {
    let v = v.trim();
    if let Some(rest) = v.strip_prefix('[') {
        // IPv6 literal: keep inside the brackets (rejected later as a hostname).
        return rest.split(']').next().unwrap_or_default().to_owned();
    }
    v.rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit()))
        .map_or(v, |(h, _)| h)
        .to_owned()
}
