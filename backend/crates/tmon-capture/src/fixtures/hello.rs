//! TLS ClientHello builders (handshake message and record).

/// Size of the X25519MLKEM768 key share (1184 + 32 bytes) browsers now send.
pub const PQ_KEY_SHARE_LEN: usize = 1216;
const EXT_SNI: u16 = 0;
const EXT_ALPN: u16 = 16;
const EXT_SUPPORTED_VERSIONS: u16 = 43;
const EXT_KEY_SHARE: u16 = 51;
const EXT_ECH: u16 = 0xfe0d;
const EXT_PADDING: u16 = 21;
/// Chrome pads post-quantum hellos to roughly this many bytes.
const PQ_PADDING_LEN: usize = 400;
const GROUP_X25519_MLKEM768: u16 = 0x11ec;

/// What to put in a ClientHello.
#[derive(Debug, Clone, Default)]
pub struct Hello<'a> {
    pub sni: Option<&'a str>,
    pub alpn: &'a [&'a str],
    pub ech: bool,
    /// Add a post-quantum key share (~1.2 KB), as Chrome/Firefox do.
    pub pq_key_share: bool,
}

fn ext(out: &mut Vec<u8>, ty: u16, body: &[u8]) {
    out.extend_from_slice(&ty.to_be_bytes());
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(body);
}

fn u16_prefixed(body: &[u8]) -> Vec<u8> {
    let mut v = (body.len() as u16).to_be_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

fn extensions(h: &Hello<'_>) -> Vec<u8> {
    let mut exts = Vec::new();
    if let Some(name) = h.sni {
        let mut entry = vec![0u8];
        entry.extend_from_slice(&u16_prefixed(name.as_bytes()));
        ext(&mut exts, EXT_SNI, &u16_prefixed(&entry));
    }
    if !h.alpn.is_empty() {
        let mut list = Vec::new();
        for p in h.alpn {
            list.push(p.len() as u8);
            list.extend_from_slice(p.as_bytes());
        }
        ext(&mut exts, EXT_ALPN, &u16_prefixed(&list));
    }
    ext(&mut exts, EXT_SUPPORTED_VERSIONS, &[2, 0x03, 0x04]);
    if h.pq_key_share {
        let mut share = GROUP_X25519_MLKEM768.to_be_bytes().to_vec();
        share.extend_from_slice(&u16_prefixed(&vec![0x5a; PQ_KEY_SHARE_LEN]));
        ext(&mut exts, EXT_KEY_SHARE, &u16_prefixed(&share));
        ext(&mut exts, EXT_PADDING, &[0; PQ_PADDING_LEN]);
    }
    if h.ech {
        // outer ClientHello type (0), cipher suite, config id, enc, payload.
        let mut body = vec![0u8, 0x00, 0x01, 0x00, 0x01, 0x07];
        body.extend_from_slice(&u16_prefixed(&[0x11; 32]));
        body.extend_from_slice(&u16_prefixed(&[0x22; 64]));
        ext(&mut exts, EXT_ECH, &body);
    }
    exts
}

/// Handshake message: type 1 (ClientHello), 24-bit length, body.
pub fn client_hello(h: &Hello<'_>) -> Vec<u8> {
    let mut body = vec![0x03, 0x03];
    body.extend_from_slice(&[0x42; 32]); // random
    body.push(32);
    body.extend_from_slice(&[0x24; 32]); // legacy session id
    body.extend_from_slice(&u16_prefixed(&[0x13, 0x01, 0x13, 0x02]));
    body.extend_from_slice(&[1, 0]); // compression: null
    body.extend_from_slice(&u16_prefixed(&extensions(h)));
    let len = body.len() as u32;
    let mut msg = vec![1u8];
    msg.extend_from_slice(&len.to_be_bytes()[1..]);
    msg.extend_from_slice(&body);
    msg
}

/// TLS record (handshake, legacy version 3.1) around one handshake message.
pub fn record(handshake: &[u8]) -> Vec<u8> {
    let mut out = vec![0x16, 0x03, 0x01];
    out.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    out.extend_from_slice(handshake);
    out
}
