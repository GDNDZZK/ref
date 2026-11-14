//! The key of this run: `--key`, or the login token kept in the state directory. Browsers
//! sign in with it; the channel is derived from it (`channel`).

use anyhow::Context as _;

use crate::channel::{Root, SALT_LEN, Salt, from_hex_array};
use crate::token;

/// Shorter keys are too easy to guess from a recorded handshake.
const MIN_KEY_LEN: usize = 16;

pub struct Key {
    text: String,
    salt: Salt,
    root: Root,
}

impl Key {
    pub fn new(text: String, salt: Salt) -> Key {
        let root = Root::derive(text.as_bytes(), &salt);
        Key { text, salt, root }
    }

    /// The key as the user types it; goes into the address ref-web prints.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn salt(&self) -> &Salt {
        &self.salt
    }

    pub fn root(&self) -> &Root {
        &self.root
    }
}

/// With `--key` the token file is neither read nor created.
pub async fn load(explicit: Option<&str>) -> anyhow::Result<Key> {
    let text = match explicit {
        Some(key) => {
            validate(key)?;
            key.to_string()
        }
        None => token::load_or_create(token::LOGIN_TOKEN).await?,
    };
    let salt_hex = token::load_or_create(token::SALT).await?;
    let salt = from_hex_array::<SALT_LEN>(&salt_hex).context("the salt file is not valid hex")?;
    // scrypt takes a noticeable time: keep it off the async threads.
    tokio::task::spawn_blocking(move || Key::new(text, salt))
        .await
        .context("key derivation failed")
}

fn validate(key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        key.chars().count() >= MIN_KEY_LEN,
        "--key must have at least {MIN_KEY_LEN} characters. Leave it out to use the random \
         login token (kept in <state dir>/web/login-token)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_keys_are_refused() {
        assert!(validate(&"a".repeat(MIN_KEY_LEN)).is_ok());
        let error = validate(&"a".repeat(MIN_KEY_LEN - 1))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("at least 16") && error.contains("Leave it out"),
            "{error}"
        );
        assert!(validate("").is_err());
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        assert!(validate(&"é".repeat(MIN_KEY_LEN)).is_ok());
        assert!(validate(&"é".repeat(MIN_KEY_LEN - 1)).is_err());
    }

    #[test]
    fn the_root_depends_on_key_and_salt() {
        let key = Key::new("k".repeat(MIN_KEY_LEN), [1; SALT_LEN]);
        let other_salt = Key::new("k".repeat(MIN_KEY_LEN), [2; SALT_LEN]);
        let other_key = Key::new("j".repeat(MIN_KEY_LEN), [1; SALT_LEN]);
        let same = Key::new("k".repeat(MIN_KEY_LEN), [1; SALT_LEN]);
        assert_eq!(key.root().to_hex(), same.root().to_hex());
        assert_ne!(key.root().to_hex(), other_salt.root().to_hex());
        assert_ne!(key.root().to_hex(), other_key.root().to_hex());
    }
}
