//! The login token (and random ids): created on first start, kept in
//! `<state_dir>/web/login-token` so that bookmarks survive restarts. Delete the file to
//! rotate it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use rand::RngCore as _;

const TOKEN_FILE: &str = "login-token";
const TOKEN_BYTES: usize = 32;
const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;
/// How long to wait for a concurrently starting ref-web to finish writing the file.
const WRITE_RACE_RETRIES: u32 = 20;
const WRITE_RACE_PAUSE: Duration = Duration::from_millis(50);
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;

pub fn state_subdir() -> PathBuf {
    ref_proto::state_dir().join("web")
}

/// Reads the login token, creating it when there is none.
pub async fn load_or_create() -> anyhow::Result<String> {
    load_or_create_in(&state_subdir()).await
}

async fn load_or_create_in(dir: &Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let path = dir.join(TOKEN_FILE);
    for _ in 0..WRITE_RACE_RETRIES {
        match std::fs::read_to_string(&path) {
            Ok(text) if is_token(text.trim()) => {
                restrict_to_owner(&path)?;
                return Ok(text.trim().to_string());
            }
            // Empty or cut off: another ref-web is writing it right now.
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(token) = try_create(&path)? {
                    return Ok(token);
                }
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
        tokio::time::sleep(WRITE_RACE_PAUSE).await;
    }
    anyhow::bail!(
        "{} does not hold a valid login token ({TOKEN_HEX_LEN} hex digits); delete it to create a new one",
        path.display()
    )
}

fn is_token(text: &str) -> bool {
    text.len() == TOKEN_HEX_LEN && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The token grants access to every session: make sure only the owner can read the file.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 == 0 {
        return Ok(());
    }
    eprintln!(
        "ref-web: warning: {} was readable by other users (mode {:o}); changed to {PRIVATE_FILE_MODE:o}",
        path.display(),
        mode & 0o777
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .with_context(|| format!("cannot restrict {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Creates the file exclusively. `None` when someone else created it first.
fn try_create(path: &Path) -> anyhow::Result<Option<String>> {
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
    let token = random_hex();
    file.write_all(token.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(Some(token))
}

/// 32 random bytes as hex.
pub fn random_hex() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
        assert!(is_token(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn only_full_length_hex_is_a_token() {
        assert!(is_token(&"ab".repeat(TOKEN_BYTES)));
        assert!(!is_token(&"ab".repeat(TOKEN_BYTES - 1)));
        assert!(!is_token(&"ab".repeat(TOKEN_BYTES + 1)));
        assert!(!is_token(&"zz".repeat(TOKEN_BYTES)));
        assert!(!is_token(""));
    }

    #[tokio::test]
    async fn creates_once_and_reuses() {
        let dir = temp_dir("create");
        let first = load_or_create_in(&dir).await.unwrap();
        assert!(is_token(&first));
        assert_eq!(load_or_create_in(&dir).await.unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn new_file_is_private_and_a_wide_one_is_restricted() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = temp_dir("mode");
        let token = load_or_create_in(&dir).await.unwrap();
        let file = dir.join(TOKEN_FILE);
        assert_eq!(mode(&file), PRIVATE_FILE_MODE);

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(load_or_create_in(&dir).await.unwrap(), token);
        assert_eq!(mode(&file), PRIVATE_FILE_MODE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_cut_off_token_is_an_error_after_the_retries() {
        let dir = temp_dir("cut");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(TOKEN_FILE), "abcd").unwrap();
        let error = load_or_create_in(&dir).await.err().unwrap().to_string();
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
        let file = dir.join(TOKEN_FILE);
        std::fs::write(&file, "abcd").unwrap();
        let full = "cd".repeat(TOKEN_BYTES);
        let writer = {
            let (file, full) = (file.clone(), full.clone());
            tokio::spawn(async move {
                tokio::time::sleep(WRITE_RACE_PAUSE * 3).await;
                std::fs::write(file, full).unwrap();
            })
        };
        assert_eq!(load_or_create_in(&dir).await.unwrap(), full);
        writer.await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
