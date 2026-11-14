//! The encrypted channel between the page and ref-web (DESIGN-0.2.1 §7.4): key derivation,
//! the handshake messages and the sealed frames. Pure functions and no I/O, so the
//! integration tests include this file (`#[path]`) to act as a browser.
// Tests use the client half, the server uses the other half.
#![cfg_attr(test, allow(dead_code))]

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const PROTOCOL_VERSION: u32 = 1;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 16;
pub const KEY_LEN: usize = 32;
pub const MAC_LEN: usize = 32;

/// scrypt cost: 2^15 blocks of 128 * r bytes = 32 MiB per guess.
const SCRYPT_LOG_N: u8 = 15;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

const AUTH_LABEL: &[u8] = b"ref-web auth v1";
const CHANNEL_LABEL: &[u8] = b"ref-web channel v1";
const PING_LABEL: &[u8] = b"ref-web ping v1";
const HELLO_TYPE: &str = "hello";
const AUTH_TYPE: &str = "auth";

/// XChaCha20 nonces are 24 bytes: 16 zero bytes, then the frame counter (big endian).
const XNONCE_LEN: usize = 24;
pub(crate) const TAG_LEN: usize = 16;

pub type Salt = [u8; SALT_LEN];
pub type Nonce = [u8; NONCE_LEN];

/// Whether a frame is a WebSocket text or binary message. It is authenticated with the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text = 0,
    Binary = 1,
}

const KINDS: [Kind; 2] = [Kind::Text, Kind::Binary];

/// A frame that does not open: wrong key, modified, dropped, repeated or reordered.
#[derive(Debug, PartialEq, Eq)]
pub struct BadFrame;

/// The key stretched with scrypt. It is what the browser stores, never the key itself.
#[derive(Clone)]
pub struct Root(pub(crate) [u8; KEY_LEN]);

impl Root {
    /// Takes seconds in an unoptimised build; call it once and keep the result.
    pub fn derive(key: &[u8], salt: &Salt) -> Root {
        let params = scrypt::Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P, KEY_LEN)
            .expect("the scrypt parameters are valid");
        let mut root = [0u8; KEY_LEN];
        scrypt::scrypt(key, salt, &params, &mut root).expect("the output length is valid");
        Root(root)
    }

    #[cfg(test)]
    pub fn from_hex(text: &str) -> Option<Root> {
        Some(Root(from_hex_array(text)?))
    }

    #[cfg(test)]
    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    fn expand<const N: usize>(&self, salt: &[u8], info: &[u8]) -> [u8; N] {
        let mut out = [0u8; N];
        Hkdf::<Sha256>::new(Some(salt), &self.0)
            .expand(info, &mut out)
            .expect("the output is shorter than 255 hash blocks");
        out
    }

    pub(crate) fn auth_key(&self) -> [u8; KEY_LEN] {
        self.expand(&[], AUTH_LABEL)
    }

    /// What the browser sends to show it knows the key.
    #[cfg(test)]
    pub fn auth_mac(&self, sn: &Nonce, cn: &Nonce) -> [u8; MAC_LEN] {
        hmac_sha256(&self.auth_key(), &[AUTH_LABEL, sn, cn])
    }

    pub fn verify_auth(&self, sn: &Nonce, cn: &Nonce, mac: &[u8]) -> bool {
        let mut check = hmac_new(&self.auth_key());
        for part in [AUTH_LABEL, sn, cn] {
            check.update(part);
        }
        check.verify_slice(mac).is_ok()
    }

    /// Answer to `GET /api/ping?nonce=`: shows the same key without revealing anything of it.
    pub fn ping_proof(&self, nonce: &str) -> [u8; MAC_LEN] {
        hmac_sha256(&self.auth_key(), &[PING_LABEL, nonce.as_bytes()])
    }

    /// The channel as the server sees it: it seals to the page and opens what the page sends.
    pub fn server_channel(&self, sn: &Nonce, cn: &Nonce) -> (Sealer, Opener) {
        let (c2s, s2c) = self.channel_keys(sn, cn);
        (Sealer::new(&s2c), Opener::new(&c2s))
    }

    #[cfg(test)]
    pub fn client_channel(&self, sn: &Nonce, cn: &Nonce) -> (Sealer, Opener) {
        let (c2s, s2c) = self.channel_keys(sn, cn);
        (Sealer::new(&c2s), Opener::new(&s2c))
    }

    /// `(client to server, server to client)`. Both nonces are fresh per connection, so every
    /// connection has keys of its own.
    pub fn channel_keys(&self, sn: &Nonce, cn: &Nonce) -> ([u8; KEY_LEN], [u8; KEY_LEN]) {
        let both: [u8; KEY_LEN * 2] = self.expand(&[sn.as_slice(), cn].concat(), CHANNEL_LABEL);
        let (c2s, s2c) = both.split_at(KEY_LEN);
        (
            c2s.try_into().expect("half of 64 bytes"),
            s2c.try_into().expect("half of 64 bytes"),
        )
    }
}

fn hmac_new(key: &[u8]) -> Hmac<Sha256> {
    <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts keys of any length")
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; MAC_LEN] {
    let mut mac = hmac_new(key);
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

fn xnonce(counter: u64) -> [u8; XNONCE_LEN] {
    let mut nonce = [0u8; XNONCE_LEN];
    nonce[XNONCE_LEN - 8..].copy_from_slice(&counter.to_be_bytes());
    nonce
}

/// Seals the frames of one direction. The counter is the nonce and is not sent.
pub struct Sealer {
    cipher: XChaCha20Poly1305,
    counter: u64,
}

impl Sealer {
    fn new(key: &[u8; KEY_LEN]) -> Sealer {
        Sealer {
            cipher: XChaCha20Poly1305::new(key.into()),
            counter: 0,
        }
    }

    /// `ciphertext ‖ tag`.
    pub fn seal(&mut self, kind: Kind, plaintext: &[u8]) -> Vec<u8> {
        let nonce = xnonce(self.counter);
        self.counter += 1;
        let payload = Payload {
            msg: plaintext,
            aad: &[kind as u8],
        };
        self.cipher
            .encrypt(XNonce::from_slice(&nonce), payload)
            .expect("sealing a message in memory cannot fail")
    }
}

/// Opens the frames of one direction, in order.
pub struct Opener {
    cipher: XChaCha20Poly1305,
    counter: u64,
}

impl Opener {
    fn new(key: &[u8; KEY_LEN]) -> Opener {
        Opener {
            cipher: XChaCha20Poly1305::new(key.into()),
            counter: 0,
        }
    }

    /// The frame must be the next one. The kind is not sent, it is found by trying both.
    pub fn open(&mut self, wire: &[u8]) -> Result<(Kind, Vec<u8>), BadFrame> {
        if wire.len() < TAG_LEN {
            return Err(BadFrame);
        }
        let nonce = xnonce(self.counter);
        for kind in KINDS {
            let payload = Payload {
                msg: wire,
                aad: &[kind as u8],
            };
            if let Ok(plaintext) = self.cipher.decrypt(XNonce::from_slice(&nonce), payload) {
                self.counter += 1;
                return Ok((kind, plaintext));
            }
        }
        Err(BadFrame)
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

pub fn from_hex_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    from_hex(text)?.try_into().ok()
}

#[derive(Serialize, Deserialize)]
struct Hello {
    t: String,
    v: u32,
    salt: String,
    sn: String,
}

#[derive(Serialize, Deserialize)]
struct Auth {
    t: String,
    cn: String,
    mac: String,
}

pub fn hello_json(salt: &Salt, sn: &Nonce) -> String {
    let hello = Hello {
        t: HELLO_TYPE.to_string(),
        v: PROTOCOL_VERSION,
        salt: to_hex(salt),
        sn: to_hex(sn),
    };
    serde_json::to_string(&hello).expect("plain data serialises")
}

#[cfg(test)]
pub fn parse_hello(text: &str) -> Option<(Salt, Nonce)> {
    let hello: Hello = serde_json::from_str(text).ok()?;
    if hello.t != HELLO_TYPE || hello.v != PROTOCOL_VERSION {
        return None;
    }
    Some((from_hex_array(&hello.salt)?, from_hex_array(&hello.sn)?))
}

#[cfg(test)]
pub fn auth_json(cn: &Nonce, mac: &[u8; MAC_LEN]) -> String {
    let auth = Auth {
        t: AUTH_TYPE.to_string(),
        cn: to_hex(cn),
        mac: to_hex(mac),
    };
    serde_json::to_string(&auth).expect("plain data serialises")
}

/// The client nonce and the MAC of an auth message; `None` when it is malformed.
pub fn parse_auth(text: &str) -> Option<(Nonce, Vec<u8>)> {
    let auth: Auth = serde_json::from_str(text).ok()?;
    if auth.t != AUTH_TYPE {
        return None;
    }
    Some((from_hex_array(&auth.cn)?, from_hex(&auth.mac)?))
}
