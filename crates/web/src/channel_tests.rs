//! Tests of `channel`: the shared vectors (`fixtures/channel.json`, also read by the page's
//! vitest) and the frame rules.

use serde::Deserialize;

use crate::channel::TAG_LEN;
use crate::channel::*;

const FIXTURE: &str = include_str!("../fixtures/channel.json");

#[derive(Deserialize)]
struct Frame {
    counter: u64,
    kind: String,
    plain: String,
    wire: String,
}

#[derive(Deserialize)]
struct Fixture {
    key: String,
    salt: String,
    root: String,
    k_auth: String,
    sn: String,
    cn: String,
    mac: String,
    k_c2s: String,
    k_s2c: String,
    ping_nonce: String,
    ping_proof: String,
    c2s: Vec<Frame>,
    s2c: Vec<Frame>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).unwrap()
}

fn nonce(text: &str) -> Nonce {
    from_hex_array(text).unwrap()
}

fn kind(name: &str) -> Kind {
    match name {
        "text" => Kind::Text,
        "binary" => Kind::Binary,
        other => panic!("unknown kind {other}"),
    }
}

#[test]
fn fixture_derivation_matches() {
    let f = fixture();
    let root = Root::derive(f.key.as_bytes(), &nonce(&f.salt));
    assert_eq!(root.to_hex(), f.root);
    assert_eq!(to_hex(&root.auth_key()), f.k_auth);
    let (sn, cn) = (nonce(&f.sn), nonce(&f.cn));
    assert_eq!(to_hex(&root.auth_mac(&sn, &cn)), f.mac);
    let (c2s, s2c) = root.channel_keys(&sn, &cn);
    assert_eq!((to_hex(&c2s), to_hex(&s2c)), (f.k_c2s, f.k_s2c));
    assert_eq!(to_hex(&root.ping_proof(&f.ping_nonce)), f.ping_proof);
}

/// The fixture frames are the first frames of each direction, from counter 0 on.
#[test]
fn fixture_frames_open_and_seal_to_the_same_bytes() {
    let f = fixture();
    let root = Root::from_hex(&f.root).unwrap();
    let (sn, cn) = (nonce(&f.sn), nonce(&f.cn));
    let (mut server_seal, mut server_open) = root.server_channel(&sn, &cn);
    let (mut client_seal, mut client_open) = root.client_channel(&sn, &cn);
    for (i, frame) in f.c2s.iter().enumerate() {
        assert_eq!(frame.counter, i as u64);
        let wire = from_hex(&frame.wire).unwrap();
        let opened = server_open.open(&wire).unwrap();
        assert_eq!(
            opened,
            (kind(&frame.kind), frame.plain.clone().into_bytes())
        );
        let sealed = client_seal.seal(kind(&frame.kind), frame.plain.as_bytes());
        assert_eq!(to_hex(&sealed), frame.wire);
    }
    for (i, frame) in f.s2c.iter().enumerate() {
        assert_eq!(frame.counter, i as u64);
        let wire = from_hex(&frame.wire).unwrap();
        let opened = client_open.open(&wire).unwrap();
        assert_eq!(
            opened,
            (kind(&frame.kind), frame.plain.clone().into_bytes())
        );
        let sealed = server_seal.seal(kind(&frame.kind), frame.plain.as_bytes());
        assert_eq!(to_hex(&sealed), frame.wire);
    }
}

fn pair() -> ((Sealer, Opener), (Sealer, Opener)) {
    let root = Root([7; KEY_LEN]);
    let (sn, cn) = ([1; NONCE_LEN], [2; NONCE_LEN]);
    (root.client_channel(&sn, &cn), root.server_channel(&sn, &cn))
}

#[test]
fn frames_round_trip_with_their_kind() {
    let ((mut seal, _), (_, mut open)) = pair();
    for (kind, data) in [
        (Kind::Text, &b"hello"[..]),
        (Kind::Binary, &[0u8, 255, 1][..]),
        (Kind::Text, &b""[..]),
    ] {
        let wire = seal.seal(kind, data);
        assert_eq!(wire.len(), data.len() + TAG_LEN);
        assert_eq!(open.open(&wire).unwrap(), (kind, data.to_vec()));
    }
}

#[test]
fn modified_repeated_dropped_and_reordered_frames_do_not_open() {
    let ((mut seal, _), (_, mut open)) = pair();
    let first = seal.seal(Kind::Text, b"one");
    let second = seal.seal(Kind::Text, b"two");
    let third = seal.seal(Kind::Text, b"three");

    let mut flipped = first.clone();
    flipped[0] ^= 1;
    assert_eq!(open.open(&flipped), Err(BadFrame));
    assert_eq!(open.open(&first[..first.len() - 1]), Err(BadFrame));
    assert_eq!(open.open(&[]), Err(BadFrame));
    // Out of order: the second frame while the first is expected.
    assert_eq!(open.open(&second), Err(BadFrame));
    assert!(open.open(&first).is_ok());
    // Repeated.
    assert_eq!(open.open(&first), Err(BadFrame));
    // Dropped: the third while the second is expected.
    assert_eq!(open.open(&third), Err(BadFrame));
    assert!(open.open(&second).is_ok());
    assert!(open.open(&third).is_ok());
}

#[test]
fn directions_and_connections_have_separate_keys() {
    let root = Root([7; KEY_LEN]);
    let (c2s, s2c) = root.channel_keys(&[1; NONCE_LEN], &[2; NONCE_LEN]);
    assert_ne!(c2s, s2c);
    let (other, _) = root.channel_keys(&[1; NONCE_LEN], &[3; NONCE_LEN]);
    assert_ne!(c2s, other);
    // A frame sent to the server does not open as a frame for the page.
    let ((mut seal, mut client_open), _) = pair();
    assert_eq!(
        client_open.open(&seal.seal(Kind::Text, b"x")),
        Err(BadFrame)
    );
}

#[test]
fn auth_mac_checks_both_nonces_and_the_key() {
    let root = Root([7; KEY_LEN]);
    let (sn, cn) = ([1; NONCE_LEN], [2; NONCE_LEN]);
    let mac = root.auth_mac(&sn, &cn);
    assert!(root.verify_auth(&sn, &cn, &mac));
    assert!(!root.verify_auth(&cn, &sn, &mac));
    assert!(!root.verify_auth(&sn, &[9; NONCE_LEN], &mac));
    assert!(!root.verify_auth(&sn, &cn, &mac[..MAC_LEN - 1]));
    assert!(!Root([8; KEY_LEN]).verify_auth(&sn, &cn, &mac));
}

#[test]
fn handshake_messages_round_trip_and_reject_garbage() {
    let (salt, sn) = ([3; SALT_LEN], [4; NONCE_LEN]);
    assert_eq!(parse_hello(&hello_json(&salt, &sn)), Some((salt, sn)));
    let mac = [5; MAC_LEN];
    let text = auth_json(&sn, &mac);
    assert_eq!(parse_auth(&text), Some((sn, mac.to_vec())));
    for bad in [
        "",
        "{}",
        r#"{"t":"auth","cn":"zz","mac":"00"}"#,
        r#"{"t":"hello","cn":"00","mac":"00"}"#,
        r#"{"t":"auth","cn":"00","mac":"00"}"#,
    ] {
        assert_eq!(parse_auth(bad), None, "{bad}");
    }
    assert_eq!(
        parse_hello(r#"{"t":"hello","v":2,"salt":"","sn":""}"#),
        None
    );
}

#[test]
fn hex_helpers() {
    assert_eq!(to_hex(&[0, 15, 255]), "000fff");
    assert_eq!(from_hex("000fFF"), Some(vec![0, 15, 255]));
    assert_eq!(from_hex("0"), None);
    assert_eq!(from_hex("zz"), None);
    assert_eq!(from_hex("é"), None);
}
