//! The secret files of ref-web: the login token (the default key) and the salt, created on
//! first start in `<state_dir>/web/` so that they survive restarts. Delete a file to get a
//! new value. Also random hex ids.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use rand::RngCore as _;

const TOKEN_BYTES: usize = 32;
/// How long to wait for a concurrently starting ref-web to finish writing the file.
const WRITE_RACE_RETRIES: u32 = 20;
const WRITE_RACE_PAUSE: Duration = Duration::from_millis(50);
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;
/// The permission bits for group and others, and all permission bits of a mode.
#[cfg(unix)]
const GROUP_OTHER_BITS: u32 = 0o077;
#[cfg(unix)]
const PERMISSION_BITS: u32 = 0o777;

/// A random value kept in a file of its own.
#[derive(Clone, Copy)]
pub struct Secret {
    file: &'static str,
    /// For messages.
    what: &'static str,
    bytes: usize,
}

impl Secret {
    fn hex_len(&self) -> usize {
        self.bytes * 2
    }

    fn is_valid(&self, text: &str) -> bool {
        text.len() == self.hex_len() && text.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// The key when `--key` is not given.
pub const LOGIN_TOKEN: Secret = Secret {
    file: "login-token",
    what: "login token",
    bytes: TOKEN_BYTES,
};

/// Public; it keeps a table of weak keys from being built once for every installation.
pub const SALT: Secret = Secret {
    file: "salt",
    what: "salt",
    bytes: crate::channel::SALT_LEN,
};

pub fn state_subdir() -> PathBuf {
    ref_proto::state_dir().join("web")
}

/// Reads the value, creating it when there is none.
pub async fn load_or_create(secret: Secret) -> anyhow::Result<String> {
    load_or_create_in(&state_subdir(), secret).await
}

async fn load_or_create_in(dir: &Path, secret: Secret) -> anyhow::Result<String> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let path = dir.join(secret.file);
    for _ in 0..WRITE_RACE_RETRIES {
        match std::fs::read_to_string(&path) {
            Ok(text) if secret.is_valid(text.trim()) => {
                restrict_to_owner(&path)?;
                return Ok(text.trim().to_string());
            }
            // Empty or cut off: another ref-web is writing it right now.
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(value) = try_create(&path, secret)? {
                    return Ok(value);
                }
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
        tokio::time::sleep(WRITE_RACE_PAUSE).await;
    }
    anyhow::bail!(
        "{} does not hold a valid {} ({} hex digits); delete it to create a new one",
        path.display(),
        secret.what,
        secret.hex_len()
    )
}

/// The login token grants access to every session: make sure only the owner can read the file.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?
        .permissions()
        .mode();
    if mode & GROUP_OTHER_BITS == 0 {
        return Ok(());
    }
    say_err!(
        "warning: {} was readable by other users (mode {:o}); changed to {PRIVATE_FILE_MODE:o}",
        path.display(),
        mode & PERMISSION_BITS
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .with_context(|| format!("cannot restrict {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Creates the file exclusively. `None` when someone else created it first.
fn try_create(path: &Path, secret: Secret) -> anyhow::Result<Option<String>> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(PRIVATE_FILE_MODE);
    }
    let mut file = match options.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot create {}", path.display())),
    };
    let value = random_hex_of(secret.bytes);
    file.write_all(value.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(Some(value))
}

/// 32 random bytes as hex.
pub fn random_hex() -> String {
    random_hex_of(TOKEN_BYTES)
}

fn random_hex_of(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::rng().fill_bytes(&mut bytes);
    crate::channel::to_hex(&bytes)
}

/// Fresh random bytes, for nonces.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ref-web-token-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn random_tokens_are_hex_and_distinct() {
        let (a, b) = (random_hex(), random_hex());
        assert!(LOGIN_TOKEN.is_valid(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn only_full_length_hex_is_a_token() {
        let is_token = |text: &str| LOGIN_TOKEN.is_valid(text);
        assert!(is_token(&"ab".repeat(TOKEN_BYTES)));
        assert!(!is_token(&"ab".repeat(TOKEN_BYTES - 1)));
        assert!(!is_token(&"ab".repeat(TOKEN_BYTES + 1)));
        assert!(!is_token(&"zz".repeat(TOKEN_BYTES)));
        assert!(!is_token(""));
    }

    #[tokio::test]
    async fn creates_once_and_reuses() {
        let dir = temp_dir("create");
        let first = load_or_create_in(&dir, LOGIN_TOKEN).await.unwrap();
        assert!(LOGIN_TOKEN.is_valid(&first));
        assert_eq!(load_or_create_in(&dir, LOGIN_TOKEN).await.unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn new_file_is_private_and_a_wide_one_is_restricted() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = temp_dir("mode");
        let token = load_or_create_in(&dir, LOGIN_TOKEN).await.unwrap();
        let file = dir.join(LOGIN_TOKEN.file);
        assert_eq!(mode(&file), PRIVATE_FILE_MODE);

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(load_or_create_in(&dir, LOGIN_TOKEN).await.unwrap(), token);
        assert_eq!(mode(&file), PRIVATE_FILE_MODE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_salt_is_16_bytes_in_a_file_of_its_own() {
        let dir = temp_dir("salt");
        let salt = load_or_create_in(&dir, SALT).await.unwrap();
        assert_eq!(salt.len(), 32);
        assert_eq!(load_or_create_in(&dir, SALT).await.unwrap(), salt);
        let token = load_or_create_in(&dir, LOGIN_TOKEN).await.unwrap();
        assert_ne!(token, salt);
        assert_eq!(std::fs::read_to_string(dir.join("salt")).unwrap(), salt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_cut_off_token_is_an_error_after_the_retries() {
        let dir = temp_dir("cut");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOGIN_TOKEN.file), "abcd").unwrap();
        let error = load_or_create_in(&dir, LOGIN_TOKEN)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("does not hold a valid login token"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_token_that_is_still_being_written_is_waited_for() {
        let dir = temp_dir("race");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(LOGIN_TOKEN.file);
        std::fs::write(&file, "abcd").unwrap();
        let full = "cd".repeat(TOKEN_BYTES);
        let writer = {
            let (file, full) = (file.clone(), full.clone());
            tokio::spawn(async move {
                tokio::time::sleep(WRITE_RACE_PAUSE * 3).await;
                std::fs::write(file, full).unwrap();
            })
        };
        assert_eq!(load_or_create_in(&dir, LOGIN_TOKEN).await.unwrap(), full);
        writer.await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
