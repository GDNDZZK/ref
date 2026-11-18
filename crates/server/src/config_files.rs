//! Crash-safe global config profile management (`ref.json` and `ref.json.<n>.bak`).

use crate::api::ApiError;
use crate::hub::Hub;
use ref_proto::{ConfigFileEntry, ConfigFilesInfo, ERR_NOT_FOUND};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

const ACTIVE_FILE: &str = "ref.json";
const JSONC_OVERRIDE_FILE: &str = "ref.jsonc";
const BAK_PREFIX: &str = "ref.json.";
const BAK_SUFFIX: &str = ".bak";
const TMP_SUFFIX: &str = ".tmp.json";
const MAX_BACKUP_NUM: u32 = 999_999;
const EMPTY_CONFIG_TEXT: &str = "{}\n";
const LABEL_KEY: &str = "$name";
const ENV_OPEN: &str = "{env:";
const INVALID_UTF8_SUMMARY: &str = "invalid UTF-8";
const INVALID_UTF8_ERROR: &str = "File is not valid UTF-8";

#[cfg(unix)]
const DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileName {
    Active,
    Backup(u32),
}

/// Removes leftover `*.tmp.json` regular files in `dir` on server startup.
/// Never follows symlinks.
pub(crate) fn cleanup_incomplete(dir: &Path) {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::debug!("cannot stat config dir {}: {e}", dir.display());
            return;
        }
    };
    let ft = meta.file_type();
    if ft.is_symlink() || !ft.is_dir() {
        tracing::warn!(
            "skipping cleanup on non-regular config dir {}",
            dir.display()
        );
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!("cannot read config dir {}: {e}", dir.display());
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() || !ft.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.ends_with(TMP_SUFFIX) {
            continue;
        }
        let path = entry.path();
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                "cannot remove incomplete config temp {}: {e}",
                path.display()
            );
        }
    }
}

/// Atomically writes `bytes` to `target` via sibling `<name>.tmp.json` + fsync + rename.
/// Refuses symlinks and non-regular files at both `target` and `<name>.tmp.json`.
pub(crate) fn write_cow_json(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| format!("invalid config path: {}", target.display()))?;
    ensure_config_dir(parent)?;
    reject_non_regular_target(target)?;

    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("invalid config file name: {}", target.display()))?;
    let tmp = parent.join(format!("{file_name}{TMP_SUFFIX}"));
    remove_stale_tmp(&tmp)?;

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(FILE_MODE);
    }
    let mut file = opts
        .open(&tmp)
        .map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
    let write_res = (|| -> std::io::Result<()> {
        use std::io::Write;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
    })();
    drop(file);
    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot write {}: {e}", tmp.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "cannot rename {} to {}: {e}",
            tmp.display(),
            target.display()
        ));
    }
    #[cfg(unix)]
    if let Ok(dir_file) = std::fs::File::open(parent) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

fn ensure_config_dir(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() || !ft.is_dir() {
                return Err(format!(
                    "config directory {} must be a regular directory",
                    dir.display()
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(DIR_MODE))
                    .map_err(|e| format!("cannot set permissions on {}: {e}", dir.display()))?;
            }
            Ok(())
        }
        Err(e) => Err(format!("cannot stat {}: {e}", dir.display())),
    }
}

fn reject_non_regular_target(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() || !ft.is_file() {
                return Err(format!(
                    "refusing to overwrite non-regular file {}",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot stat {}: {e}", path.display())),
    }
}

fn remove_stale_tmp(tmp: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(tmp) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() || !ft.is_file() {
                return Err(format!(
                    "refusing to overwrite non-regular temporary file {}",
                    tmp.display()
                ));
            }
            std::fs::remove_file(tmp)
                .map_err(|e| format!("cannot remove stale {}: {e}", tmp.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot stat {}: {e}", tmp.display())),
    }
}

fn parse_profile_name(name: &str) -> Result<ProfileName, ApiError> {
    if name == ACTIVE_FILE {
        return Ok(ProfileName::Active);
    }
    let invalid = || ApiError::from(format!("Invalid config profile file name: {name:?}"));
    let mid = name
        .strip_prefix(BAK_PREFIX)
        .and_then(|s| s.strip_suffix(BAK_SUFFIX))
        .ok_or_else(invalid)?;
    if mid.is_empty() || mid.starts_with('0') || !mid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let num: u32 = mid.parse().map_err(|_| invalid())?;
    if !(1..=MAX_BACKUP_NUM).contains(&num) {
        return Err(invalid());
    }
    Ok(ProfileName::Backup(num))
}

fn regular_metadata_opt(path: &Path, label: &str) -> Result<Option<std::fs::Metadata>, ApiError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() || !ft.is_file() {
                return Err(format!("Refusing to access non-regular config file: {label}").into());
            }
            Ok(Some(meta))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot stat {label}: {e}").into()),
    }
}

fn regular_metadata_req(path: &Path, label: &str) -> Result<std::fs::Metadata, ApiError> {
    regular_metadata_opt(path, label)?
        .ok_or_else(|| ApiError::coded(ERR_NOT_FOUND, format!("Config profile not found: {label}")))
}

fn modified_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn verify_regular_config_dir(dir: &Path) -> Result<bool, ApiError> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() || !ft.is_dir() {
                return Err(format!(
                    "Config directory {} must be a regular directory.",
                    dir.display()
                )
                .into());
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Cannot stat {}: {e}", dir.display()).into()),
    }
}

fn list_backups(dir: &Path) -> Result<Vec<(u32, String, PathBuf, std::fs::Metadata)>, ApiError> {
    if !verify_regular_config_dir(dir)? {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| ApiError::from(format!("Cannot read {}: {e}", dir.display())))?;
    let mut backups = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Ok(ProfileName::Backup(num)) = parse_profile_name(name) else {
            continue;
        };
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.file_type().is_file() {
            continue;
        }
        backups.push((num, name.to_string(), path, meta));
    }
    backups.sort_by_key(|(num, _, _, _)| *num);
    Ok(backups)
}

fn next_backup_num(dir: &Path) -> Result<u32, ApiError> {
    let backups = list_backups(dir)?;
    let max_num = backups.last().map_or(0, |(n, _, _, _)| *n);
    if max_num >= MAX_BACKUP_NUM {
        return Err(format!("Maximum backup count ({MAX_BACKUP_NUM}) reached.").into());
    }
    Ok(max_num + 1)
}

fn parse_raw_object(raw: &str) -> (Value, Option<String>) {
    match ref_config::parse_jsonc(raw) {
        Ok(v @ Value::Object(_)) => (v, None),
        Ok(_) => (Value::Null, Some("config must be a JSON object".into())),
        Err(e) => (Value::Null, Some(e.to_string())),
    }
}

fn require_object(raw: &str) -> Result<Map<String, Value>, ApiError> {
    match ref_config::parse_jsonc(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("Config must be a JSON object.".into()),
        Err(e) => Err(format!("Invalid JSONC: {e}").into()),
    }
}

fn extract_label(parsed: &Value) -> Option<String> {
    parsed
        .get(LABEL_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn summarize_profile(exists: bool, parsed: &Value, parse_error: Option<&str>) -> String {
    if parse_error.is_some() {
        return "syntax error".into();
    }
    if !exists {
        return "not created yet".into();
    }
    let Some(obj) = parsed.as_object() else {
        return "empty".into();
    };
    let mut parts = Vec::new();
    if let Some(providers) = obj.get("provider").and_then(Value::as_object)
        && !providers.is_empty()
    {
        let n = providers.len();
        let noun = if n == 1 { "provider" } else { "providers" };
        parts.push(format!("{n} {noun}"));
    }
    if let Some(model) = obj
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(model.to_string());
    }
    if let Some(mcp) = obj.get("mcp").and_then(Value::as_object)
        && !mcp.is_empty()
    {
        parts.push(format!("{} mcp", mcp.len()));
    }
    if parts.is_empty()
        && let Some(agents) = obj.get("agent").and_then(Value::as_object)
        && !agents.is_empty()
    {
        let n = agents.len();
        let noun = if n == 1 { "agent" } else { "agents" };
        parts.push(format!("{n} {noun}"));
    }
    if parts.is_empty() {
        "empty".into()
    } else {
        parts.join(" · ")
    }
}

fn collect_env_vars(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) => collect_env_vars_in_str(s, out),
        Value::Array(items) => {
            for item in items {
                collect_env_vars(item, out);
            }
        }
        Value::Object(map) => {
            for val in map.values() {
                collect_env_vars(val, out);
            }
        }
        _ => {}
    }
}

fn collect_env_vars_in_str(mut rest: &str, out: &mut BTreeSet<String>) {
    while let Some(start) = rest.find(ENV_OPEN) {
        let after_open = &rest[start + ENV_OPEN.len()..];
        let Some(end) = after_open.find('}') else {
            break;
        };
        let var = after_open[..end].trim();
        if !var.is_empty() {
            out.insert(var.to_string());
        }
        rest = &after_open[end + 1..];
    }
}

fn build_env_status(parsed: &Value, is_mock: bool) -> BTreeMap<String, bool> {
    build_env_status_with(parsed, is_mock, |var| std::env::var(var).ok())
}

fn build_env_status_with(
    parsed: &Value,
    is_mock: bool,
    lookup: impl Fn(&str) -> Option<String>,
) -> BTreeMap<String, bool> {
    let mut vars = BTreeSet::new();
    collect_env_vars(parsed, &mut vars);
    vars.into_iter()
        .map(|var| {
            let present = !is_mock && lookup(&var).is_some_and(|v| !v.trim().is_empty());
            (var, present)
        })
        .collect()
}

fn env_config_override(is_mock: bool) -> Option<String> {
    if is_mock {
        return None;
    }
    for key in ["REF_CONFIG", "OPENCODE_CONFIG"] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn global_dirs_for_overrides(
    config_dir: &Path,
    home: Option<&Path>,
    is_mock: bool,
) -> Vec<PathBuf> {
    let mut dirs = vec![config_dir.to_path_buf()];
    if is_mock {
        return dirs;
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")));
    if let Some(oc_dir) = std::env::var_os("OPENCODE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(oc_dir));
    } else if let Some(b) = &base {
        dirs.push(b.join("opencode"));
    }
    if let Some(b) = &base {
        let ref_dir = b.join("ref");
        if ref_dir != config_dir {
            dirs.push(ref_dir);
        }
    }
    dirs
}

struct DecodedProfile {
    raw_text: String,
    parsed: Value,
    parse_error: Option<String>,
    invalid_utf8: bool,
}

fn decode_profile_bytes(bytes: &[u8]) -> DecodedProfile {
    match std::str::from_utf8(bytes) {
        Ok(raw) => {
            let (parsed, parse_error) = parse_raw_object(raw);
            DecodedProfile {
                raw_text: raw.to_string(),
                parsed,
                parse_error,
                invalid_utf8: false,
            }
        }
        Err(_) => DecodedProfile {
            raw_text: String::from_utf8_lossy(bytes).into_owned(),
            parsed: Value::Null,
            parse_error: Some(INVALID_UTF8_ERROR.to_string()),
            invalid_utf8: true,
        },
    }
}

fn build_entry(
    file: String,
    active: bool,
    meta: Option<&std::fs::Metadata>,
    decoded: &DecodedProfile,
) -> ConfigFileEntry {
    let exists = meta.is_some();
    let summary = if decoded.invalid_utf8 {
        INVALID_UTF8_SUMMARY.to_string()
    } else {
        summarize_profile(exists, &decoded.parsed, decoded.parse_error.as_deref())
    };
    ConfigFileEntry {
        file,
        active,
        exists,
        label: extract_label(&decoded.parsed),
        summary,
        modified_at: meta.map_or(0, modified_ms),
        size: meta.map_or(0, std::fs::Metadata::len),
    }
}

fn require_config_dir(hub: &Hub) -> Result<PathBuf, ApiError> {
    hub.opts
        .config_dir
        .clone()
        .ok_or_else(|| ApiError::from("Global config directory is not configured."))
}

async fn build_info(
    hub: &Arc<Hub>,
    config_dir: PathBuf,
    selected_file: String,
    cwd: String,
) -> Result<Value, ApiError> {
    let hub_clone = hub.clone();
    let info = tokio::task::spawn_blocking(move || {
        let profile = parse_profile_name(&selected_file)?;
        verify_regular_config_dir(&config_dir)?;
        let active_path = config_dir.join(ACTIVE_FILE);
        let active_meta = regular_metadata_opt(&active_path, ACTIVE_FILE)?;
        let active_decoded = match &active_meta {
            Some(_) => {
                let bytes = std::fs::read(&active_path)
                    .map_err(|e| ApiError::from(format!("Cannot read {ACTIVE_FILE}: {e}")))?;
                decode_profile_bytes(&bytes)
            }
            None => DecodedProfile {
                raw_text: EMPTY_CONFIG_TEXT.to_string(),
                parsed: json!({}),
                parse_error: None,
                invalid_utf8: false,
            },
        };

        let backups = list_backups(&config_dir)?;
        if matches!(profile, ProfileName::Backup(_))
            && !backups.iter().any(|(_, name, _, _)| name == &selected_file)
        {
            // Surface the exact error (missing -> ERR_NOT_FOUND, symlink/dir -> refused).
            regular_metadata_req(&config_dir.join(&selected_file), &selected_file)?;
        }

        let mut files = Vec::with_capacity(1 + backups.len());
        files.push(build_entry(
            ACTIVE_FILE.to_string(),
            true,
            active_meta.as_ref(),
            &active_decoded,
        ));

        let mut selected_decoded = (profile == ProfileName::Active).then_some(active_decoded);

        for (_, name, path, meta) in &backups {
            let bytes = std::fs::read(path)
                .map_err(|e| ApiError::from(format!("Cannot read {name}: {e}")))?;
            let decoded = decode_profile_bytes(&bytes);
            files.push(build_entry(name.clone(), false, Some(meta), &decoded));
            if name == &selected_file {
                selected_decoded = Some(decoded);
            }
        }

        let selected = selected_decoded.ok_or_else(|| {
            ApiError::coded(
                ERR_NOT_FOUND,
                format!("Config profile not found: {selected_file}"),
            )
        })?;

        let is_mock = hub_clone.opts.sandbox_cwd.is_some();
        let env_status = build_env_status(&selected.parsed, is_mock);
        let has_jsonc_override =
            regular_metadata_opt(&config_dir.join(JSONC_OVERRIDE_FILE), JSONC_OVERRIDE_FILE)?
                .is_some();
        let env_override = env_config_override(is_mock);
        let home = if is_mock {
            None
        } else {
            hub_clone.opts.home.as_deref()
        };
        let globals = global_dirs_for_overrides(&config_dir, home, is_mock);
        let project_overrides = ref_config::project_config_files(
            home,
            &globals,
            hub_clone.opts.sandbox_cwd.as_deref(),
            Path::new(&cwd),
        )
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

        let merged = hub_clone.load_config(Path::new(&cwd));
        Ok::<_, ApiError>(ConfigFilesInfo {
            config_dir: config_dir.to_string_lossy().into_owned(),
            files,
            selected_file,
            raw_text: selected.raw_text,
            parsed: selected.parsed,
            parse_error: selected.parse_error,
            env_status,
            has_jsonc_override,
            env_config_override: env_override,
            project_overrides,
            warnings: merged.warnings,
        })
    })
    .await
    .map_err(|e| ApiError::from(format!("reading config files failed: {e}")))??;

    Ok(serde_json::to_value(info).expect("ConfigFilesInfo serializes"))
}

pub(crate) async fn get_config_files(
    hub: &Arc<Hub>,
    file: Option<String>,
    cwd: String,
) -> Result<Value, ApiError> {
    let config_dir = require_config_dir(hub)?;
    let _guard = hub.config_files_lock.lock().await;
    let selected = file
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| ACTIVE_FILE.to_string());
    build_info(hub, config_dir, selected, cwd).await
}

pub(crate) async fn save_config_file(
    hub: &Arc<Hub>,
    file: String,
    content: String,
    cwd: String,
) -> Result<Value, ApiError> {
    let config_dir = require_config_dir(hub)?;
    let profile = parse_profile_name(&file)?;
    require_object(&content)?;

    let _guard = hub.config_files_lock.lock().await;
    let target = config_dir.join(&file);
    let bytes = content.into_bytes();
    tokio::task::spawn_blocking(move || write_cow_json(&target, &bytes))
        .await
        .map_err(|e| ApiError::from(format!("saving config failed: {e}")))?
        .map_err(ApiError::from)?;

    if profile == ProfileName::Active {
        crate::ops::reload_all(hub, Some(&cwd)).await?;
    }
    build_info(hub, config_dir, file, cwd).await
}

pub(crate) async fn create_config_file(
    hub: &Arc<Hub>,
    label: Option<String>,
    clone_current: bool,
    content: Option<String>,
    cwd: String,
) -> Result<Value, ApiError> {
    let config_dir = require_config_dir(hub)?;
    let _guard = hub.config_files_lock.lock().await;
    let dir_for_write = config_dir.clone();

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        verify_regular_config_dir(&dir_for_write)?;
        let active_path = dir_for_write.join(ACTIVE_FILE);
        let active_meta = regular_metadata_opt(&active_path, ACTIVE_FILE)?;
        let existing_bytes = match &active_meta {
            Some(_) => Some(
                std::fs::read(&active_path)
                    .map_err(|e| ApiError::from(format!("Cannot read {ACTIVE_FILE}: {e}")))?,
            ),
            None => None,
        };

        let mut map = if let Some(raw) = content {
            require_object(&raw)?
        } else if clone_current {
            match &existing_bytes {
                Some(bytes) => {
                    let raw = std::str::from_utf8(bytes).map_err(|e| {
                        ApiError::from(format!("Invalid UTF-8 in {ACTIVE_FILE}: {e}"))
                    })?;
                    require_object(raw)?
                }
                None => Map::new(),
            }
        } else {
            Map::new()
        };

        if let Some(lbl) = label {
            let trimmed = lbl.trim();
            if trimmed.is_empty() {
                map.remove(LABEL_KEY);
            } else {
                map.insert(LABEL_KEY.to_string(), Value::String(trimmed.to_string()));
            }
        }

        let mut new_text =
            serde_json::to_string_pretty(&Value::Object(map)).expect("object serializes");
        new_text.push('\n');

        if let Some(old_bytes) = existing_bytes {
            let next = next_backup_num(&dir_for_write)?;
            let bak_path = dir_for_write.join(format!("{BAK_PREFIX}{next}{BAK_SUFFIX}"));
            write_cow_json(&bak_path, &old_bytes)?;
        }
        write_cow_json(&active_path, new_text.as_bytes())?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::from(format!("creating config failed: {e}")))??;

    crate::ops::reload_all(hub, Some(&cwd)).await?;
    build_info(hub, config_dir, ACTIVE_FILE.to_string(), cwd).await
}

pub(crate) async fn select_config_file(
    hub: &Arc<Hub>,
    file: String,
    cwd: String,
) -> Result<Value, ApiError> {
    let config_dir = require_config_dir(hub)?;
    let profile = parse_profile_name(&file)?;
    let _guard = hub.config_files_lock.lock().await;

    if profile == ProfileName::Active {
        return build_info(hub, config_dir, ACTIVE_FILE.to_string(), cwd).await;
    }

    let dir_for_swap = config_dir.clone();
    let selected_name = file.clone();
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        if !verify_regular_config_dir(&dir_for_swap)? {
            return Err(ApiError::coded(
                ERR_NOT_FOUND,
                format!("Config profile not found: {selected_name}"),
            ));
        }
        let bak_path = dir_for_swap.join(&selected_name);
        regular_metadata_req(&bak_path, &selected_name)?;
        let selected_bytes = std::fs::read(&bak_path)
            .map_err(|e| ApiError::from(format!("Cannot read {selected_name}: {e}")))?;
        let selected_text = std::str::from_utf8(&selected_bytes)
            .map_err(|e| ApiError::from(format!("Invalid UTF-8 in {selected_name}: {e}")))?;
        require_object(selected_text)?;

        let active_path = dir_for_swap.join(ACTIVE_FILE);
        if regular_metadata_opt(&active_path, ACTIVE_FILE)?.is_some() {
            let current_bytes = std::fs::read(&active_path)
                .map_err(|e| ApiError::from(format!("Cannot read {ACTIVE_FILE}: {e}")))?;
            let next = next_backup_num(&dir_for_swap)?;
            let next_bak = dir_for_swap.join(format!("{BAK_PREFIX}{next}{BAK_SUFFIX}"));
            write_cow_json(&next_bak, &current_bytes)?;
        }

        write_cow_json(&active_path, &selected_bytes)?;
        std::fs::remove_file(&bak_path)
            .map_err(|e| ApiError::from(format!("Cannot remove {selected_name}: {e}")))?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError::from(format!("selecting config failed: {e}")))??;

    crate::ops::reload_all(hub, Some(&cwd)).await?;
    build_info(hub, config_dir, ACTIVE_FILE.to_string(), cwd).await
}

struct ValidBackup<'a> {
    name: &'a str,
    path: &'a Path,
    bytes: Vec<u8>,
}

fn newest_valid_backup(
    backups: &[(u32, String, PathBuf, std::fs::Metadata)],
) -> Result<Option<ValidBackup<'_>>, ApiError> {
    for (_, name, path, _) in backups.iter().rev() {
        let bytes =
            std::fs::read(path).map_err(|e| ApiError::from(format!("Cannot read {name}: {e}")))?;
        let is_valid = std::str::from_utf8(&bytes)
            .ok()
            .is_some_and(|raw| require_object(raw).is_ok());
        if !is_valid {
            tracing::warn!("skipping corrupt config backup {name}");
            continue;
        }
        return Ok(Some(ValidBackup {
            name: name.as_str(),
            path: path.as_path(),
            bytes,
        }));
    }
    Ok(None)
}

pub(crate) async fn delete_config_file(
    hub: &Arc<Hub>,
    file: String,
    cwd: String,
) -> Result<Value, ApiError> {
    let config_dir = require_config_dir(hub)?;
    let profile = parse_profile_name(&file)?;
    let _guard = hub.config_files_lock.lock().await;

    let dir_for_del = config_dir.clone();
    let target_name = file.clone();
    let reload_needed = tokio::task::spawn_blocking(move || -> Result<bool, ApiError> {
        let dir_exists = verify_regular_config_dir(&dir_for_del)?;
        match profile {
            ProfileName::Backup(_) => {
                if !dir_exists {
                    return Err(ApiError::coded(
                        ERR_NOT_FOUND,
                        format!("Config profile not found: {target_name}"),
                    ));
                }
                let bak_path = dir_for_del.join(&target_name);
                regular_metadata_req(&bak_path, &target_name)?;
                std::fs::remove_file(&bak_path)
                    .map_err(|e| ApiError::from(format!("Cannot remove {target_name}: {e}")))?;
                Ok(false)
            }
            ProfileName::Active => {
                if !dir_exists {
                    return Ok(false);
                }
                let active_path = dir_for_del.join(ACTIVE_FILE);
                let active_exists = regular_metadata_opt(&active_path, ACTIVE_FILE)?.is_some();
                let backups = list_backups(&dir_for_del)?;
                if let Some(cand) = newest_valid_backup(&backups)? {
                    write_cow_json(&active_path, &cand.bytes)?;
                    std::fs::remove_file(cand.path)
                        .map_err(|e| ApiError::from(format!("Cannot remove {}: {e}", cand.name)))?;
                    Ok(true)
                } else if active_exists {
                    std::fs::remove_file(&active_path)
                        .map_err(|e| ApiError::from(format!("Cannot remove {ACTIVE_FILE}: {e}")))?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    })
    .await
    .map_err(|e| ApiError::from(format!("deleting config failed: {e}")))??;

    if reload_needed {
        crate::ops::reload_all(hub, Some(&cwd)).await?;
    }
    build_info(hub, config_dir, ACTIVE_FILE.to_string(), cwd).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ref-cfg-unit-{tag}-{}-{}",
            std::process::id(),
            crate::util::random_hex(6)
        ))
    }

    #[test]
    fn profile_name_validation() {
        assert_eq!(parse_profile_name("ref.json"), Ok(ProfileName::Active));
        assert_eq!(
            parse_profile_name("ref.json.1.bak"),
            Ok(ProfileName::Backup(1))
        );
        assert_eq!(
            parse_profile_name("ref.json.42.bak"),
            Ok(ProfileName::Backup(42))
        );
        assert_eq!(
            parse_profile_name("ref.json.999999.bak"),
            Ok(ProfileName::Backup(999_999))
        );

        for bad in [
            "",
            "ref.jsonc",
            "opencode.json",
            "ref.json.0.bak",
            "ref.json.01.bak",
            "ref.json.+1.bak",
            "ref.json.-1.bak",
            "ref.json.1000000.bak",
            "ref.json.abc.bak",
            "../ref.json",
            "sub/ref.json",
            "ref.json.tmp.json",
            "ref.json.1.bak.tmp.json",
        ] {
            assert!(parse_profile_name(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn cleanup_incomplete_removes_only_regular_tmp_json_files() {
        let dir = tmp_dir("cleanup");
        std::fs::create_dir_all(&dir).unwrap();
        let keep_active = dir.join("ref.json");
        let keep_bak = dir.join("ref.json.1.bak");
        let stale_1 = dir.join("ref.json.tmp.json");
        let stale_2 = dir.join("ref.json.1.bak.tmp.json");
        let dir_tmp = dir.join("nested.tmp.json");
        std::fs::write(&keep_active, "{}\n").unwrap();
        std::fs::write(&keep_bak, "{}\n").unwrap();
        std::fs::write(&stale_1, "partial").unwrap();
        std::fs::write(&stale_2, "partial").unwrap();
        std::fs::create_dir_all(&dir_tmp).unwrap();

        #[cfg(unix)]
        let (sym_tmp, sym_target) = {
            let target = dir.join("outside.json");
            std::fs::write(&target, "important").unwrap();
            let link = dir.join("link.tmp.json");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            (link, target)
        };

        cleanup_incomplete(&dir);

        assert!(keep_active.exists());
        assert!(keep_bak.exists());
        assert!(!stale_1.exists());
        assert!(!stale_2.exists());
        assert!(dir_tmp.is_dir());
        #[cfg(unix)]
        {
            assert!(sym_tmp.symlink_metadata().is_ok());
            assert_eq!(std::fs::read_to_string(&sym_target).unwrap(), "important");
            // Symlinked config_dir itself must never be traversed by cleanup_incomplete.
            let real_dir = dir.join("real-cfg");
            std::fs::create_dir_all(&real_dir).unwrap();
            let inner_tmp = real_dir.join("ref.json.tmp.json");
            std::fs::write(&inner_tmp, "keep-behind-symlink").unwrap();
            let sym_dir = dir.join("sym-cfg");
            std::os::unix::fs::symlink(&real_dir, &sym_dir).unwrap();
            cleanup_incomplete(&sym_dir);
            assert!(inner_tmp.exists());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_cow_json_sets_permissions_cleans_stale_tmp_and_rejects_symlinks() {
        let root = tmp_dir("cow");
        let cfg_dir = root.join("config");
        let target = cfg_dir.join("ref.json");
        write_cow_json(&target, b"{\"a\":1}\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{\"a\":1}\n");
        assert!(!cfg_dir.join("ref.json.tmp.json").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&cfg_dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        // Stale regular .tmp.json is replaced cleanly.
        std::fs::write(cfg_dir.join("ref.json.tmp.json"), "stale").unwrap();
        write_cow_json(&target, b"{\"a\":2}\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{\"a\":2}\n");
        assert!(!cfg_dir.join("ref.json.tmp.json").exists());

        #[cfg(unix)]
        {
            let outside = root.join("secret.txt");
            std::fs::write(&outside, "keep").unwrap();
            let sym_target = cfg_dir.join("ref.json.1.bak");
            std::os::unix::fs::symlink(&outside, &sym_target).unwrap();
            assert!(write_cow_json(&sym_target, b"{}").is_err());
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");

            let sym_tmp = cfg_dir.join("ref.json.tmp.json");
            std::os::unix::fs::symlink(&outside, &sym_tmp).unwrap();
            assert!(write_cow_json(&target, b"{}").is_err());
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn env_scanning_and_profile_summaries() {
        let parsed = json!({
            "$name": "  Work  ",
            "model": "anthropic/claude-sonnet-4-5",
            "provider": {
                "anthropic": { "options": { "apiKey": "{env:PATH}" } },
                "openai": { "options": { "headers": { "X": "{env:MISSING_REF_TEST_ENV_999} {env:PATH}" } } }
            }
        });
        assert_eq!(extract_label(&parsed).as_deref(), Some("Work"));
        assert_eq!(
            summarize_profile(true, &parsed, None),
            "2 providers · anthropic/claude-sonnet-4-5"
        );
        assert_eq!(
            summarize_profile(false, &json!({}), None),
            "not created yet"
        );
        assert_eq!(
            summarize_profile(true, &Value::Null, Some("bad")),
            "syntax error"
        );

        let real_env = build_env_status(&parsed, false);
        assert_eq!(real_env.get("PATH"), Some(&true));
        assert_eq!(real_env.get("MISSING_REF_TEST_ENV_999"), Some(&false));

        let mock_env = build_env_status(&parsed, true);
        assert_eq!(mock_env.get("PATH"), Some(&false));

        let ws_doc =
            json!({ "a": "{env:BLANK_VAR}", "b": "{env:PADDED_VAR}", "c": "{env:EMPTY_VAR}" });
        let ws_env = build_env_status_with(&ws_doc, false, |k| match k {
            "BLANK_VAR" => Some("   \t\n ".into()),
            "PADDED_VAR" => Some("  sk-live  ".into()),
            "EMPTY_VAR" => Some(String::new()),
            _ => None,
        });
        assert_eq!(ws_env.get("BLANK_VAR"), Some(&false));
        assert_eq!(ws_env.get("PADDED_VAR"), Some(&true));
        assert_eq!(ws_env.get("EMPTY_VAR"), Some(&false));
    }
}
