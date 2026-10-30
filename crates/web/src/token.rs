//! The login token: created on first start, kept in `<state_dir>/web/login-token` so that
//! bookmarks and the browser cookie survive restarts. Delete the file to rotate it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use rand::RngCore as _;

const TOKEN_FILE: &str = "login-token";
const TOKEN_BYTES: usize = 32;
/// How long to wait for a concurrently starting ref-web to finish writing the file.
const WRITE_RACE_RETRIES: u32 = 20;
const WRITE_RACE_PAUSE: Duration = Duration::from_millis(50);

pub fn state_subdir() -> PathBuf {
    ref_proto::state_dir().join("web")
}

/// Reads the login token, creating it when there is none.
pub fn load_or_create() -> anyhow::Result<String> {
    let dir = state_subdir();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let path = dir.join(TOKEN_FILE);
    for _ in 0..WRITE_RACE_RETRIES {
        match std::fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => return validated(text.trim(), &path),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(token) = try_create(&path)? {
                    return Ok(token);
                }
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
        // Another ref-web created the file but has not written it yet.
        std::thread::sleep(WRITE_RACE_PAUSE);
    }
    anyhow::bail!("{} stays empty; delete it and retry", path.display())
}

fn validated(token: &str, path: &Path) -> anyhow::Result<String> {
    let valid = token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    anyhow::ensure!(
        valid,
        "{} holds characters that are not allowed in a login token; delete it to create a new one",
        path.display()
    );
    Ok(token.to_string())
}

/// Creates the file exclusively. `None` when someone else created it first.
fn try_create(path: &Path) -> anyhow::Result<Option<String>> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot create {}", path.display())),
    };
    let token = random_token();
    file.write_all(token.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(Some(token))
}

fn random_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_tokens_are_hex_and_distinct() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), TOKEN_BYTES * 2);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_unsafe_characters() {
        assert!(validated("abc-DEF_123", Path::new("f")).is_ok());
        assert!(validated("a;b", Path::new("f")).is_err());
        assert!(validated("a b", Path::new("f")).is_err());
    }
}
