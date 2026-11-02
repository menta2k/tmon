//! QUIC Initial key schedule (RFC 9001 §5.2, RFC 9369 §3.3.1) on `ring`.

use ring::aead::{self, LessSafeKey, UnboundKey, quic};
use ring::hkdf::{self, HKDF_SHA256, KeyType, Prk, Salt};

const SALT_V1: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
    0xcc, 0xbb, 0x7f, 0x0a,
];
const SALT_V2: [u8; 20] = [
    0x0d, 0xed, 0xe3, 0xde, 0xf7, 0x00, 0xa6, 0xdb, 0x81, 0x93, 0x81, 0xbe, 0x6e, 0x26, 0x9d, 0xcb,
    0xf9, 0xbd, 0x2e, 0xd9,
];
pub const KEY_LEN: usize = 16;
pub const IV_LEN: usize = 12;
const SECRET_LEN: usize = 32;

/// QUIC versions whose Initial packets we can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V1,
    V2,
}

impl Version {
    pub const WIRE_V1: u32 = 0x0000_0001;
    pub const WIRE_V2: u32 = 0x6b33_43cf;

    pub fn from_wire(v: u32) -> Option<Self> {
        match v {
            Self::WIRE_V1 => Some(Self::V1),
            Self::WIRE_V2 => Some(Self::V2),
            _ => None,
        }
    }

    pub fn wire(self) -> u32 {
        match self {
            Self::V1 => Self::WIRE_V1,
            Self::V2 => Self::WIRE_V2,
        }
    }

    /// Long-header packet type of Initial packets.
    pub fn initial_type(self) -> u8 {
        match self {
            Self::V1 => 0b00,
            Self::V2 => 0b01,
        }
    }

    fn salt(self) -> &'static [u8] {
        match self {
            Self::V1 => &SALT_V1,
            Self::V2 => &SALT_V2,
        }
    }

    fn labels(self) -> (&'static [u8], &'static [u8], &'static [u8]) {
        match self {
            Self::V1 => (b"quic key", b"quic iv", b"quic hp"),
            Self::V2 => (b"quicv2 key", b"quicv2 iv", b"quicv2 hp"),
        }
    }
}

struct Len(usize);

impl KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

/// TLS 1.3 HKDF-Expand-Label with an empty context.
fn expand_label(prk: &Prk, label: &[u8], out: &mut [u8]) {
    let full_len = 6 + label.len();
    let mut info = Vec::with_capacity(4 + full_len);
    info.extend_from_slice(&(out.len() as u16).to_be_bytes());
    info.push(full_len as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);
    info.push(0);
    let parts = [info.as_slice()];
    prk.expand(&parts, Len(out.len()))
        .and_then(|okm| okm.fill(out))
        .expect("HKDF output length is within limits");
}

/// Raw client Initial secrets (exposed for the RFC test vectors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secrets {
    pub client_secret: [u8; SECRET_LEN],
    pub key: [u8; KEY_LEN],
    pub iv: [u8; IV_LEN],
    pub hp: [u8; KEY_LEN],
}

pub fn client_secrets(version: Version, dcid: &[u8]) -> Secrets {
    let initial = Salt::new(HKDF_SHA256, version.salt()).extract(dcid);
    let mut client_secret = [0u8; SECRET_LEN];
    expand_label(&initial, b"client in", &mut client_secret);
    let prk = Prk::new_less_safe(hkdf::HKDF_SHA256, &client_secret);
    let (kl, il, hl) = version.labels();
    let mut s = Secrets {
        client_secret,
        key: [0; KEY_LEN],
        iv: [0; IV_LEN],
        hp: [0; KEY_LEN],
    };
    expand_label(&prk, kl, &mut s.key);
    expand_label(&prk, il, &mut s.iv);
    expand_label(&prk, hl, &mut s.hp);
    s
}

/// Keys to open client Initial packets of one connection.
pub struct InitialKeys {
    pub version: Version,
    pub(crate) key: LessSafeKey,
    pub(crate) iv: [u8; IV_LEN],
    pub(crate) hp: quic::HeaderProtectionKey,
}

impl InitialKeys {
    pub fn client(version: Version, dcid: &[u8]) -> Self {
        let s = client_secrets(version, dcid);
        Self {
            version,
            key: LessSafeKey::new(
                UnboundKey::new(&aead::AES_128_GCM, &s.key).expect("16-byte AES key"),
            ),
            iv: s.iv,
            hp: quic::HeaderProtectionKey::new(&quic::AES_128, &s.hp).expect("16-byte hp key"),
        }
    }
}

impl std::fmt::Debug for InitialKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InitialKeys")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
