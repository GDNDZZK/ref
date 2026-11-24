//! Persistent TUI preferences (`<state_dir>/tui.json`) and backend target resolution.

use std::path::{Path, PathBuf};

use ref_client::BackendTarget;
use serde::{Deserialize, Serialize};

/// Unix file mode for `tui.json` so any stored token stays owner-only.
#[cfg(unix)]
const PREFS_FILE_MODE: u32 = 0o600;
const PREFS_FILE_NAME: &str = "tui.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendSource {
    Cli,
    Saved,
    Auto,
    Temporary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiPrefs {
    #[serde(
        rename = "backendUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub backend_url: Option<String>,
    #[serde(
        rename = "backendToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub backend_token: Option<String>,
}

pub fn prefs_path() -> Result<PathBuf, String> {
    Ok(ref_proto::state_dir()?.join(PREFS_FILE_NAME))
}

pub fn load_prefs_from(path: &Path) -> Result<TuiPrefs, String> {
    if !path.exists() {
        return Ok(TuiPrefs::default());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(TuiPrefs::default());
    }
    serde_json::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
}

pub fn load_prefs() -> Result<TuiPrefs, String> {
    let path = prefs_path()?;
    load_prefs_from(&path)
}

pub fn save_prefs_to(path: &Path, prefs: &TuiPrefs) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let mut json = serde_json::to_string_pretty(prefs)
        .map_err(|e| format!("cannot encode preferences: {e}"))?;
    json.push('\n');
    write_private(path, json.as_bytes())
}

pub fn save_prefs(prefs: &TuiPrefs) -> Result<(), String> {
    let path = prefs_path()?;
    save_prefs_to(&path, prefs)
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(PREFS_FILE_MODE)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(PREFS_FILE_MODE))
        .map_err(|e| format!("cannot set permissions on {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.flush())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn normalize_token(token: Option<&str>) -> Option<String> {
    token
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Resolves the startup backend target with precedence: CLI > saved `tui.json` > local auto.
pub fn resolve_target(
    cli_server: Option<&str>,
    cli_token: Option<&str>,
    prefs: &TuiPrefs,
) -> Result<(BackendTarget, BackendSource), String> {
    if let Some(url) = cli_server {
        let trimmed = url.trim().to_string();
        ref_client::validate_explicit_url(&trimmed).map_err(|e| e.to_string())?;
        return Ok((
            BackendTarget::Explicit {
                url: trimmed,
                token: normalize_token(cli_token),
            },
            BackendSource::Cli,
        ));
    }
    if let Some(url) = prefs.backend_url.as_deref() {
        let trimmed = url.trim().to_string();
        if !trimmed.is_empty() {
            ref_client::validate_explicit_url(&trimmed).map_err(|e| e.to_string())?;
            return Ok((
                BackendTarget::Explicit {
                    url: trimmed,
                    token: normalize_token(prefs.backend_token.as_deref()),
                },
                BackendSource::Saved,
            ));
        }
    }
    Ok((BackendTarget::Auto, BackendSource::Auto))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_prefs_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ref-tui-prefs-{name}-{}-{nanos}/tui.json",
            std::process::id()
        ))
    }

    #[test]
    fn missing_prefs_returns_default_and_save_roundtrips() {
        let path = temp_prefs_path("roundtrip");
        let dir = path.parent().unwrap().to_path_buf();
        assert_eq!(load_prefs_from(&path).unwrap(), TuiPrefs::default());

        let prefs = TuiPrefs {
            backend_url: Some("wss://remote.example/ws".into()),
            backend_token: Some("secret-token".into()),
        };
        save_prefs_to(&path, &prefs).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"backendUrl\""), "{raw}");
        assert!(raw.contains("\"backendToken\""), "{raw}");
        assert_eq!(load_prefs_from(&path).unwrap(), prefs);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, PREFS_FILE_MODE);
        }

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resolve_target_precedence_cli_over_saved_over_auto() {
        let saved = TuiPrefs {
            backend_url: Some("ws://saved:8080".into()),
            backend_token: Some("saved-tok".into()),
        };
        assert_eq!(
            resolve_target(Some("ws://cli:9090"), Some("cli-tok"), &saved).unwrap(),
            (
                BackendTarget::Explicit {
                    url: "ws://cli:9090".into(),
                    token: Some("cli-tok".into()),
                },
                BackendSource::Cli
            )
        );
        assert_eq!(
            resolve_target(None, None, &saved).unwrap(),
            (
                BackendTarget::Explicit {
                    url: "ws://saved:8080".into(),
                    token: Some("saved-tok".into()),
                },
                BackendSource::Saved
            )
        );
        assert_eq!(
            resolve_target(None, None, &TuiPrefs::default()).unwrap(),
            (BackendTarget::Auto, BackendSource::Auto)
        );
        assert!(resolve_target(Some("http://bad:8080"), None, &saved).is_err());
    }
}
