//! Integration tests for v5 config profile management, `--mock` isolation, hot reload,
//! MCP reconciliation, `ListDirs` files/cwd, `ListBinaries`, and `list_sessions` pagination.

mod common;

use common::*;
use ref_config::{AgentConfig, Config, McpConfig, ModelConfig, ProviderConfig};
use ref_proto::{
    ConfigFilesInfo, ERR_DIR_NOT_FOUND, ERR_NOT_FOUND, ModelRef, Request, ServerFrame,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn model_cfg(key: &str, context: u64, variants: &[&str]) -> ModelConfig {
    ModelConfig {
        key: key.into(),
        id: key.into(),
        name: key.into(),
        context,
        output: 4096,
        variants: variants
            .iter()
            .map(|v| ((*v).to_string(), serde_json::Map::new()))
            .collect(),
        ..Default::default()
    }
}

fn multi_model_config(url: &str, default_model: &str, models: &[(&str, u64, &[&str])]) -> Config {
    let mut cfg = config(url);
    let mut map = BTreeMap::new();
    for (k, ctx, vars) in models {
        map.insert((*k).to_string(), model_cfg(k, *ctx, vars));
    }
    cfg.providers.insert(
        "test".into(),
        ProviderConfig {
            id: "test".into(),
            name: "Test".into(),
            api: Some("chat".into()),
            base_url: Some(url.into()),
            api_key: Some("k".into()),
            models: map,
            ..Default::default()
        },
    );
    cfg.model = ModelRef::parse(default_model);
    cfg
}

#[tokio::test]
async fn default_hub_options_isolates_config_dir_and_cleans_incomplete_tmp_on_startup() {
    let dir = temp_dir("cfg-init");
    let opts = ref_server::HubOptions::new(dir.join("data"));
    assert!(opts.config_dir.is_none());
    let hub_no_cfg = ref_server::Hub::new(opts);
    let conn = hello_conn(&hub_no_cfg);
    let err = ref_server::api::handle(
        &hub_no_cfg,
        &conn,
        Request::GetConfigFiles {
            file: None,
            cwd: None,
        },
    )
    .await
    .unwrap_err();
    assert!(err.message.contains("not configured"), "{err}");

    // When config_dir is set, Hub::new removes leftover *.tmp.json files.
    let cfg_dir = dir.join("config");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("ref.json.tmp.json"), "incomplete").unwrap();
    std::fs::write(cfg_dir.join("ref.json"), "{\"$name\":\"Clean\"}\n").unwrap();
    let mut opts2 = hub_options(Config::default(), &dir);
    opts2.config_dir = Some(cfg_dir.clone());
    let _hub2 = ref_server::Hub::new(opts2);
    assert!(!cfg_dir.join("ref.json.tmp.json").exists());
    assert!(cfg_dir.join("ref.json").exists());
}

#[tokio::test]
async fn config_profiles_crud_raw_jsonc_overrides_and_broadcast() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let dir = temp_dir("cfg-crud");
    let cfg_dir = dir.join("config");
    let work = dir.join("work");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    std::fs::create_dir_all(work.join(".ref")).unwrap();
    let proj_override = work.join(".ref").join("ref.json");
    std::fs::write(&proj_override, "{}\n").unwrap();

    let cfg_dir_loader = cfg_dir.clone();
    let work_loader = work.clone();
    let mut opts = ref_server::HubOptions::new(dir.join("data"));
    opts.home = None;
    opts.config_dir = Some(cfg_dir.clone());
    opts.config_loader = Arc::new(move |cwd| {
        ref_config::load_in(
            None,
            std::slice::from_ref(&cfg_dir_loader),
            Some(&work_loader),
            cwd,
            &|_| None,
        )
    });
    let hub = ref_server::Hub::new(opts);
    let conn = hello_conn(&hub);
    conn.lock().cwd = work.to_string_lossy().into_owned();

    // 1. Fresh directory before ref.json exists on disk.
    let initial: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::GetConfigFiles {
                file: None,
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(initial.files.len(), 1);
    assert_eq!(initial.files[0].file, "ref.json");
    assert!(initial.files[0].active);
    assert!(!initial.files[0].exists);
    assert_eq!(initial.raw_text, "{}\n");
    assert_eq!(initial.parsed, json!({}));
    assert!(!initial.has_jsonc_override);
    assert_eq!(
        initial.project_overrides,
        vec![proj_override.to_string_lossy().into_owned()]
    );

    // 2. SaveConfigFile preserves JSONC comments and does NOT expand {env:...} or {file:...}.
    let raw_with_comments = format!(
        "// active profile\n{{\n  \"$name\": \"Primary\",\n  \"model\": \"p/m1\",\n  \"provider\": {{\n    \"p\": {{\n      \"api\": \"chat\",\n      \"options\": {{ \"baseURL\": \"{}\", \"apiKey\": \"{{env:PATH}}\", \"extra\": \"{{file:nonexistent.txt}}\" }},\n      \"models\": {{ \"m1\": {{ \"name\": \"M1\" }} }},\n    }},\n  }},\n}}\n",
        m.url
    );
    let _ = conn.out.drain();
    let saved: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::SaveConfigFile {
                file: "ref.json".into(),
                content: raw_with_comments.clone(),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert!(saved.files[0].exists);
    assert_eq!(saved.files[0].label.as_deref(), Some("Primary"));
    assert_eq!(saved.files[0].summary, "1 provider · p/m1");
    assert_eq!(saved.raw_text, raw_with_comments);
    assert_eq!(
        saved.parsed["provider"]["p"]["options"]["apiKey"],
        "{env:PATH}"
    );
    assert_eq!(
        saved.parsed["provider"]["p"]["options"]["extra"],
        "{file:nonexistent.txt}"
    );
    assert_eq!(saved.env_status.get("PATH"), Some(&true));
    assert!(
        conn.out
            .drain()
            .iter()
            .any(|f| matches!(f, ServerFrame::ConfigChanged { .. })),
        "saving ref.json broadcasts ConfigChanged"
    );

    // Invalid JSONC and non-object JSON are rejected without touching ref.json.
    for bad in ["{ broken", "[1, 2, 3]", "\"hello\""] {
        let err = ref_server::api::handle(
            &hub,
            &conn,
            Request::SaveConfigFile {
                file: "ref.json".into(),
                content: bad.into(),
                cwd: None,
            },
        )
        .await
        .unwrap_err();
        assert!(!err.message.is_empty());
    }
    assert_eq!(
        std::fs::read_to_string(cfg_dir.join("ref.json")).unwrap(),
        raw_with_comments
    );

    // 3. CreateConfigFile backs up ref.json -> ref.json.1.bak and writes new active ref.json.
    let created: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::CreateConfigFile {
                label: Some("Secondary".into()),
                clone_current: true,
                content: None,
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(created.files.len(), 2);
    assert_eq!(created.files[0].file, "ref.json");
    assert_eq!(created.files[0].label.as_deref(), Some("Secondary"));
    assert_eq!(created.files[1].file, "ref.json.1.bak");
    assert_eq!(created.files[1].label.as_deref(), Some("Primary"));

    // Reading missing backup returns ERR_NOT_FOUND; existing backup returns its raw content.
    let not_found = ref_server::api::handle(
        &hub,
        &conn,
        Request::GetConfigFiles {
            file: Some("ref.json.99.bak".into()),
            cwd: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(not_found.code, Some(ERR_NOT_FOUND));

    let bak_view: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::GetConfigFiles {
                file: Some("ref.json.1.bak".into()),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(bak_view.selected_file, "ref.json.1.bak");
    assert_eq!(bak_view.raw_text, raw_with_comments);

    // 4. SelectConfigFile swaps ref.json.1.bak into ref.json and backs up Secondary -> ref.json.2.bak.
    let selected: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::SelectConfigFile {
                file: "ref.json.1.bak".into(),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(selected.files.len(), 2);
    assert_eq!(selected.files[0].label.as_deref(), Some("Primary"));
    assert_eq!(selected.files[1].file, "ref.json.2.bak");
    assert_eq!(selected.files[1].label.as_deref(), Some("Secondary"));

    // ref.jsonc override detection.
    std::fs::write(cfg_dir.join("ref.jsonc"), "{}\n").unwrap();
    let with_jsonc: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::GetConfigFiles {
                file: None,
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert!(with_jsonc.has_jsonc_override);

    // 5. DeleteConfigFile on ref.json promotes newest .bak (ref.json.2.bak); second delete clears ref.json.
    let promoted: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::DeleteConfigFile {
                file: "ref.json".into(),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(promoted.files.len(), 1);
    assert_eq!(promoted.files[0].label.as_deref(), Some("Secondary"));

    let emptied: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::DeleteConfigFile {
                file: "ref.json".into(),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert!(!emptied.files[0].exists);
    assert_eq!(emptied.raw_text, "{}\n");
}

#[tokio::test]
async fn mock_mode_isolates_config_profiles_and_hides_host_env() {
    let mock = ref_server::mock::start(0, "tok".into()).await.unwrap();
    let hub = mock.running.hub.clone();
    let conn = hello_conn(&hub);
    conn.lock().cwd = mock.workspace().to_string_lossy().into_owned();

    let info: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::GetConfigFiles {
                file: None,
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert!(PathBuf::from(&info.config_dir).starts_with(mock.root()));
    assert!(info.raw_text.ends_with('\n'));
    assert!(info.env_config_override.is_none());

    // Saving a profile referencing {env:PATH} in mock mode never leaks host env status.
    let mut doc = info.parsed.clone();
    doc["$name"] = json!("Mock Edited");
    doc["tui"] = json!({ "hint": "{env:PATH}" });
    let updated: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::SaveConfigFile {
                file: "ref.json".into(),
                content: format!("{}\n", serde_json::to_string_pretty(&doc).unwrap()),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(updated.env_status.get("PATH"), Some(&false));

    // Dynamic loader in mock mode reflects tui update on GetConfig immediately.
    let cfg_res = call(&hub, &conn, Request::GetConfig { cwd: None }).await;
    assert_eq!(cfg_res["config"]["tui"], json!({ "hint": "" }));

    // ListBinaries in mock mode uses deterministic mock list and never populates host PATH cache.
    let mock_bins = call(
        &hub,
        &conn,
        Request::ListBinaries {
            query: String::new(),
            limit: Some(200),
        },
    )
    .await;
    let bin_list: Vec<String> = serde_json::from_value(mock_bins["binaries"].clone()).unwrap();
    assert!(bin_list.contains(&"git".to_string()), "{bin_list:?}");
    assert!(bin_list.contains(&"cargo".to_string()), "{bin_list:?}");
    assert!(bin_list.contains(&"echo".to_string()), "{bin_list:?}");
    assert!(
        bin_list.len() <= 40,
        "host PATH leaked into mock mode: {bin_list:?}"
    );
    hub.shutdown.cancel();
    ref_server::ops::shutdown_all(&hub).await;
    mock.running.task.abort();
}

#[tokio::test]
async fn hot_reload_reconciles_sessions_explicit_models_agents_and_mid_turn_context() {
    let m = mock(Arc::new(|_, _| Reply::Text("done".into()))).await;
    let initial_cfg = {
        let mut c = multi_model_config(
            &m.url,
            "test/m1",
            &[
                ("m1", 100_000, &["low", "high"]),
                ("m2", 200_000, &["fast"]),
            ],
        );
        c.agents.insert(
            "custom".into(),
            AgentConfig {
                name: "custom".into(),
                description: "Custom agent".into(),
                creatable: true,
                model: ModelRef::parse("test/m2"),
                ..Default::default()
            },
        );
        c
    };
    let live_cfg = Arc::new(Mutex::new(initial_cfg.clone()));
    let loader_cfg = live_cfg.clone();
    let (hub, work) = hub_with(initial_cfg, "hot-reload", |opts| {
        opts.config_loader = Arc::new(move |_| loader_cfg.lock().unwrap().clone());
    });
    let conn = hello_conn(&hub);
    conn.lock().cwd = work.to_string_lossy().into_owned();

    // s_default: created without explicit model -> tracks default model changes.
    let s_default = session(&hub, &work, "default-sess", BUILD).await;
    assert_eq!(s_default.info().model.key(), "test/m1");
    assert!(!s_default.lock().model_explicit);

    // s_explicit: created with explicit model + variant -> keeps explicit model, clears removed variant,
    // falls back only when explicit model is removed.
    let s_explicit = ref_server::ops::create_session(
        &hub,
        ref_server::ops::CreateOpts {
            cwd: work.to_string_lossy().into_owned(),
            agent: Some(BUILD.into()),
            model: ModelRef::parse("test/m1#high"),
            name: Some("explicit-sess".into()),
            unique_suffix: false,
            parent: None,
            must_report: false,
        },
    )
    .await
    .unwrap();
    assert!(s_explicit.lock().model_explicit);
    prompt(&hub, &s_explicit, "persist me");
    wait_idle(&s_explicit).await;

    // Close and reload s_explicit from SQLite store: model_explicit is preserved!
    let explicit_id = s_explicit.id.clone();
    ref_server::ops::close_session(&hub, &s_explicit).await;
    hub.store.flush().unwrap();
    let s_explicit = ref_server::ops::get_or_load(&hub, &explicit_id)
        .await
        .unwrap();
    assert!(s_explicit.lock().model_explicit);

    // s_custom: running "custom" agent with conversation history.
    let s_custom = session(&hub, &work, "custom-sess", "custom").await;
    prompt(&hub, &s_custom, "hello custom");
    wait_idle(&s_custom).await;
    assert_eq!(s_custom.info().agent, "custom");

    // Reload 1: change default model to test/m2, remove "high" variant from m1, remove "custom" agent.
    {
        let mut c = multi_model_config(
            &m.url,
            "test/m2",
            &[("m1", 120_000, &["low"]), ("m2", 250_000, &["fast"])],
        );
        c.warnings.push("sample warning".into());
        *live_cfg.lock().unwrap() = c;
    }
    let _ = conn.out.drain();
    let reload_res = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    assert_eq!(reload_res["warnings"], json!(["sample warning"]));
    assert_eq!(reload_res["config"]["warnings"], json!(["sample warning"]));

    // Default session moved to test/m2 (250k); explicit session stayed on test/m1 (120k) with variant cleared!
    assert_eq!(s_default.info().model.key(), "test/m2");
    assert_eq!(s_default.info().context.max, 250_000);
    assert_eq!(s_explicit.info().model.key(), "test/m1");
    assert_eq!(s_explicit.info().model.variant, None);
    assert_eq!(s_explicit.info().context.max, 120_000);
    assert!(s_explicit.lock().model_explicit);

    // s_custom fell back to "build" with an agent-switch reminder and adopted build's default model (test/m2).
    assert_eq!(s_custom.info().agent, BUILD);
    assert_eq!(s_custom.info().model.key(), "test/m2");
    assert!(
        messages(&s_custom)
            .last()
            .unwrap()
            .raw_text()
            .contains("switched the agent from custom to build")
    );

    // ConfigChanged frame was broadcast with warnings.
    assert!(conn.out.drain().iter().any(
        |f| matches!(f, ServerFrame::ConfigChanged { warnings } if warnings == &vec!["sample warning".to_string()])
    ));

    // Reload 2: remove test/m1 altogether -> s_explicit falls back to test/m2 and clears model_explicit.
    *live_cfg.lock().unwrap() =
        multi_model_config(&m.url, "test/m2", &[("m2", 300_000, &["fast"])]);
    let _ = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    assert_eq!(s_explicit.info().model.key(), "test/m2");
    assert!(!s_explicit.lock().model_explicit);
    assert_eq!(s_explicit.info().context.max, 300_000);
    // SetModel sets model_explicit = true; SetAgent to an agent with its own model resets model_explicit = false.
    let _ = call(
        &hub,
        &conn,
        Request::SetModel {
            session_id: s_default.id.clone(),
            model: ModelRef::parse("test/m2#fast").unwrap(),
        },
    )
    .await;
    assert!(s_default.lock().model_explicit);
    {
        let mut c = multi_model_config(&m.url, "test/m2", &[("m2", 300_000, &["fast"])]);
        c.agents.insert(
            "specialist".into(),
            AgentConfig {
                name: "specialist".into(),
                description: "Specialist".into(),
                creatable: true,
                model: ModelRef::parse("test/m2"),
                ..Default::default()
            },
        );
        *live_cfg.lock().unwrap() = c;
    }
    let _ = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    let _ = call(
        &hub,
        &conn,
        Request::SetAgent {
            session_id: s_default.id.clone(),
            agent: "specialist".into(),
        },
    )
    .await;
    assert!(!s_default.lock().model_explicit);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finalize_message_preserves_context_updated_by_hot_reload_during_stream() {
    let entered = Arc::new(AtomicBool::new(false));
    let proceed = Arc::new(AtomicBool::new(false));
    let entered_h = entered.clone();
    let proceed_h = proceed.clone();
    let m = mock(Arc::new(move |_, _| {
        entered_h.store(true, Ordering::SeqCst);
        tokio::task::block_in_place(|| {
            while !proceed_h.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        Reply::Text("hello".into())
    }))
    .await;
    let cfg_1 = multi_model_config(&m.url, "test/m1", &[("m1", 100_000, &[])]);
    let cfg_2 = multi_model_config(&m.url, "test/m2", &[("m2", 400_000, &[])]);
    let live_cfg = Arc::new(Mutex::new(cfg_1.clone()));
    let loader_cfg = live_cfg.clone();
    let (hub, work) = hub_with(cfg_1, "mid-stream-reload", |opts| {
        opts.config_loader = Arc::new(move |_| loader_cfg.lock().unwrap().clone());
    });
    let s = session(&hub, &work, "stream-sess", BUILD).await;

    prompt(&hub, &s, "go");
    wait_until("llm request started", TIMEOUT, || {
        entered.load(Ordering::SeqCst)
    })
    .await;

    *live_cfg.lock().unwrap() = cfg_2;
    ref_server::ops::reload_all(&hub, Some(&work.to_string_lossy()))
        .await
        .unwrap();
    assert_eq!(s.info().context.max, 400_000);
    proceed.store(true, Ordering::SeqCst);

    wait_idle(&s).await;
    assert_eq!(
        s.info().context.max,
        400_000,
        "finalize_message must keep hot-reloaded model context window"
    );
}

const FAKE_MCP_PY: &str = r#"
import sys, json, os

flag = os.environ["FLAG_FILE"]
if not os.path.exists(flag):
    sys.stderr.write("not ready yet\n")
    sys.stderr.flush()
    sys.exit(1)

pid_path = os.environ["PID_FILE"]
tmp_path = pid_path + ".tmp"
with open(tmp_path, "w") as f:
    f.write(str(os.getpid()))
os.replace(tmp_path, pid_path)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method")
    req_id = msg.get("id")
    if method == "initialize":
        res = {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1.0"},
        }
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": res}) + "\n")
        sys.stdout.flush()
    elif method == "tools/list":
        res = {"tools": [{"name": "ping", "description": "d", "inputSchema": {"type": "object"}}]}
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": res}) + "\n")
        sys.stdout.flush()
"#;

#[tokio::test]
async fn mcp_reconcile_retries_failed_servers_and_closes_removed_ones() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let dir = temp_dir("mcp-reconcile");
    let flag_file = dir.join("allow-mcp");
    let pid_file = dir.join("mcp.pid");

    let make_cfg = |enabled: bool| {
        let mut c = config(&m.url);
        let mut env = BTreeMap::new();
        env.insert("FLAG_FILE".into(), flag_file.to_string_lossy().into_owned());
        env.insert("PID_FILE".into(), pid_file.to_string_lossy().into_owned());
        c.mcp.insert(
            "fake".into(),
            McpConfig::Local {
                command: vec![
                    "python3".into(),
                    "-u".into(),
                    "-c".into(),
                    FAKE_MCP_PY.into(),
                ],
                environment: env,
                cwd: None,
                enabled,
                timeout_ms: Some(30_000),
            },
        );
        c
    };

    let live_cfg = Arc::new(Mutex::new(make_cfg(true)));
    let loader_cfg = live_cfg.clone();
    let (hub, work) = hub_with(make_cfg(true), "mcp-rec-hub", |opts| {
        opts.config_loader = Arc::new(move |_| loader_cfg.lock().unwrap().clone());
    });
    let conn = hello_conn(&hub);
    conn.lock().cwd = work.to_string_lossy().into_owned();

    // First status check triggers connection, which fails because flag_file does not exist.
    let _ = call(&hub, &conn, Request::McpStatus { cwd: None }).await;
    wait_until("mcp failed", TIMEOUT, || {
        let cfg = live_cfg.lock().unwrap().clone();
        hub.mcp
            .status(&cfg)
            .iter()
            .any(|s| s.name == "fake" && s.status == ref_proto::MCP_FAILED)
    })
    .await;

    // Create flag_file and run ReloadConfig -> failed server is evicted and reconnected!
    std::fs::write(&flag_file, "ok").unwrap();
    let _ = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    wait_until("mcp connected after reload", TIMEOUT, || {
        let cfg = live_cfg.lock().unwrap().clone();
        hub.mcp.status(&cfg).iter().any(|s| {
            s.name == "fake" && s.status == ref_proto::MCP_CONNECTED && s.tools == vec!["fake_ping"]
        })
    })
    .await;
    assert!(pid_file.exists());
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(ref_sys::pid_alive(pid));

    // Disable server and reload -> server is removed and child exits on stdin EOF.
    *live_cfg.lock().unwrap() = make_cfg(false);
    let _ = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    let status_after = call(&hub, &conn, Request::McpStatus { cwd: None }).await;
    assert_eq!(
        status_after["servers"][0]["status"],
        ref_proto::MCP_DISABLED
    );
    wait_until("mcp child exited after reload", TIMEOUT, || {
        !ref_sys::pid_alive(pid)
    })
    .await;
}

#[tokio::test]
async fn list_dirs_files_cwd_and_list_binaries_end_to_end() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let (hub, work) = hub(config(&m.url), "dirs-bins");
    let sub = work.join("sub");
    std::fs::create_dir_all(sub.join("Zeta")).unwrap();
    std::fs::create_dir_all(sub.join("alpha")).unwrap();
    std::fs::write(sub.join("b.txt"), "b").unwrap();
    std::fs::write(sub.join("A.rs"), "a").unwrap();

    let conn = hello_conn(&hub);
    conn.lock().cwd = work.to_string_lossy().into_owned();

    // include_files: false -> files is empty; cwd overrides conn.cwd for relative path.
    let dirs_only = call(
        &hub,
        &conn,
        Request::ListDirs {
            path: ".".into(),
            cwd: Some(sub.to_string_lossy().into_owned()),
            include_files: false,
        },
    )
    .await;
    assert_eq!(dirs_only["dirs"], json!(["alpha", "Zeta"]));
    assert_eq!(dirs_only["files"], json!([]));

    // include_files: true -> sorted files returned.
    let with_files = call(
        &hub,
        &conn,
        Request::ListDirs {
            path: ".".into(),
            cwd: Some(sub.to_string_lossy().into_owned()),
            include_files: true,
        },
    )
    .await;
    assert_eq!(with_files["dirs"], json!(["alpha", "Zeta"]));
    assert_eq!(with_files["files"], json!(["A.rs", "b.txt"]));

    // Missing directory still returns ERR_DIR_NOT_FOUND.
    let err = ref_server::api::handle(
        &hub,
        &conn,
        Request::ListDirs {
            path: "missing".into(),
            cwd: Some(sub.to_string_lossy().into_owned()),
            include_files: true,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, Some(ERR_DIR_NOT_FOUND));

    // ListBinaries returns builtins and respects limit.
    let bins = call(
        &hub,
        &conn,
        Request::ListBinaries {
            query: "ec".into(),
            limit: Some(10),
        },
    )
    .await;
    let list: Vec<String> = serde_json::from_value(bins["binaries"].clone()).unwrap();
    assert!(list.contains(&"echo".to_string()), "{list:?}");
    let echo_idx = list.iter().position(|b| b == "echo").unwrap();
    if let Some(sub_idx) = list
        .iter()
        .position(|b| !b.to_lowercase().starts_with("ec"))
    {
        assert!(
            echo_idx < sub_idx,
            "prefix matches must precede substring matches: {list:?}"
        );
    }
}

#[tokio::test]
async fn list_sessions_pagination_creator_filter_past_200_and_title_sanitization() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let (hub, work) = hub(config(&m.url), "peers-page");
    let boss = session(&hub, &work, "boss", BUILD).await;
    let long_multiline_title = format!("Line one\n\tLine two {}", "x".repeat(90));
    boss.lock().info.title = long_multiline_title;

    // Seed 205 closed history sessions: first 5 (oldest!) created by boss, newer 200 created by user.
    let cwd_str = work.to_string_lossy().into_owned();
    for idx in 0..205u32 {
        let s = ref_server::ops::create_session(
            &hub,
            ref_server::ops::CreateOpts {
                cwd: cwd_str.clone(),
                agent: Some(WORKER.into()),
                model: None,
                name: Some(format!("hist-{idx:03}")),
                unique_suffix: false,
                parent: if idx < 5 { Some(boss.id.clone()) } else { None },
                must_report: false,
            },
        )
        .await
        .unwrap();
        {
            let mut st = s.lock();
            st.messages.push(ref_server::session::user_message(
                ref_proto::MessageSource::Human,
                "hi".into(),
                vec![],
            ));
            st.info.updated_at = 1_000 + idx as i64;
        }
        ref_server::ops::close_session(&hub, &s).await;
    }
    hub.store.flush().unwrap();

    // 1. Invalid page and page_size bounds are rejected.
    for bad in [
        json!({"page": 0}),
        json!({"page_size": 0}),
        json!({"page_size": 101}),
        json!({"page": 2}),
    ] {
        let res = ref_server::peers::run(&hub, &boss, "list_sessions", &bad).await;
        assert!(res.is_error, "expected error for {bad}: {}", res.text);
    }

    // 2. Single-page live title is single-line sanitized and truncated to 80 chars + ellipsis.
    let live_res = ref_server::peers::run(&hub, &boss, "list_sessions", &json!({})).await;
    assert!(!live_res.is_error, "{}", live_res.text);
    assert!(live_res.text.starts_with("1 live session(s):\n"));
    assert!(!live_res.text.contains("Line one\n"));
    assert!(live_res.text.contains("Line one Line two "));
    assert!(live_res.text.contains('…'));

    // 3. creator filter sees sessions beyond the newest 200 history entries!
    let by_boss = ref_server::peers::run(
        &hub,
        &boss,
        "list_sessions",
        &json!({"history": true, "creator": "boss"}),
    )
    .await;
    assert!(!by_boss.is_error, "{}", by_boss.text);
    assert!(
        by_boss
            .text
            .starts_with("5 session(s) (0 live, 5 history):"),
        "{}",
        by_boss.text
    );
    assert!(by_boss.text.contains("hist-000") && by_boss.text.contains("hist-004"));

    // 4. Multi-page pagination headers and range errors.
    let p1 = ref_server::peers::run(
        &hub,
        &boss,
        "list_sessions",
        &json!({"history": true, "page": 1, "page_size": 80}),
    )
    .await;
    assert!(!p1.is_error, "{}", p1.text);
    assert!(
        p1.text.starts_with(
            "Showing 1–80 of 206 session(s) (1 live, 205 history; page 1/3, pass page: 2 for more):"
        ),
        "{}",
        p1.text
    );

    let p3 = ref_server::peers::run(
        &hub,
        &boss,
        "list_sessions",
        &json!({"history": true, "page": 3, "page_size": 80}),
    )
    .await;
    assert!(!p3.is_error, "{}", p3.text);
    assert!(
        p3.text
            .starts_with("Showing 161–206 of 206 session(s) (1 live, 205 history; page 3/3):"),
        "{}",
        p3.text
    );

    let p4 = ref_server::peers::run(
        &hub,
        &boss,
        "list_sessions",
        &json!({"history": true, "page": 4, "page_size": 80}),
    )
    .await;
    assert!(p4.is_error && p4.text.contains("1–3"), "{}", p4.text);
}

#[tokio::test]
async fn config_files_resilience_corrupt_backups_symlink_dir_and_noop_delete() {
    let dir = temp_dir("cfg-resilience");
    let cfg_dir = dir.join("config");
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();

    let mut opts = hub_options(Config::default(), &dir);
    opts.config_dir = Some(cfg_dir.clone());
    let hub = ref_server::Hub::new(opts);
    let conn = hello_conn(&hub);
    conn.lock().cwd = work.to_string_lossy().into_owned();

    // 1. DeleteConfigFile(ref.json) when neither ref.json nor backups exist is a no-op (no ConfigChanged).
    let _ = conn.out.drain();
    let noop: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::DeleteConfigFile {
                file: "ref.json".into(),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert!(!noop.files[0].exists);
    assert!(
        !conn
            .out
            .drain()
            .iter()
            .any(|f| matches!(f, ServerFrame::ConfigChanged { .. })),
        "no-op active delete must not broadcast ConfigChanged"
    );

    // 2. Corrupt non-UTF-8 .bak does not break GetConfigFiles; selecting it surfaces parse_error.
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("ref.json"), "{\"$name\":\"Active\"}\n").unwrap();
    std::fs::write(
        cfg_dir.join("ref.json.1.bak"),
        "{\"$name\":\"ValidGood\"}\n",
    )
    .unwrap();
    std::fs::write(cfg_dir.join("ref.json.2.bak"), b"\xff\xfe\x00broken").unwrap();
    std::fs::write(cfg_dir.join("ref.json.3.bak"), "[1, 2, 3]\n").unwrap();

    let listed: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::GetConfigFiles {
                file: None,
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(listed.files.len(), 4);
    assert_eq!(listed.files[0].label.as_deref(), Some("Active"));
    assert_eq!(listed.files[2].file, "ref.json.2.bak");
    assert_eq!(listed.files[2].summary, "invalid UTF-8");
    assert_eq!(listed.files[3].file, "ref.json.3.bak");
    assert_eq!(listed.files[3].summary, "syntax error");

    let bad_selected: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::GetConfigFiles {
                file: Some("ref.json.2.bak".into()),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(bad_selected.selected_file, "ref.json.2.bak");
    assert_eq!(bad_selected.parsed, serde_json::Value::Null);
    assert_eq!(
        bad_selected.parse_error.as_deref(),
        Some("File is not valid UTF-8")
    );
    assert!(bad_selected.raw_text.contains("broken"));

    // 3. DeleteConfigFile(ref.json) skips corrupt backups (2 & 3) and promotes valid ref.json.1.bak!
    let promoted: ConfigFilesInfo = serde_json::from_value(
        call(
            &hub,
            &conn,
            Request::DeleteConfigFile {
                file: "ref.json".into(),
                cwd: None,
            },
        )
        .await,
    )
    .unwrap();
    assert_eq!(promoted.files[0].label.as_deref(), Some("ValidGood"));
    assert!(!cfg_dir.join("ref.json.1.bak").exists());
    assert!(cfg_dir.join("ref.json.2.bak").exists());
    assert!(cfg_dir.join("ref.json.3.bak").exists());

    // 4. Symlinked config_dir is rejected on GetConfigFiles and DeleteConfigFile without traversing.
    #[cfg(unix)]
    {
        let outside = dir.join("outside-cfg");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("ref.json"), "{\"$name\":\"Secret\"}\n").unwrap();
        std::fs::write(outside.join("ref.json.1.bak"), "{\"$name\":\"Keep\"}\n").unwrap();
        let sym_cfg = dir.join("sym-config");
        std::os::unix::fs::symlink(&outside, &sym_cfg).unwrap();
        let mut sym_opts = hub_options(Config::default(), &dir.join("sym-data"));
        sym_opts.config_dir = Some(sym_cfg);
        let sym_hub = ref_server::Hub::new(sym_opts);
        let sym_conn = hello_conn(&sym_hub);
        sym_conn.lock().cwd = work.to_string_lossy().into_owned();
        for req in [
            Request::GetConfigFiles {
                file: None,
                cwd: None,
            },
            Request::DeleteConfigFile {
                file: "ref.json.1.bak".into(),
                cwd: None,
            },
            Request::DeleteConfigFile {
                file: "ref.json".into(),
                cwd: None,
            },
        ] {
            let err = ref_server::api::handle(&sym_hub, &sym_conn, req)
                .await
                .unwrap_err();
            assert!(err.message.contains("regular directory"), "{err}");
        }
        assert!(outside.join("ref.json").exists());
        assert!(outside.join("ref.json.1.bak").exists());
    }
}

#[tokio::test]
async fn session_reconcile_on_resume_and_cd_plus_agent_variant_persistence() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let make_rich_cfg = |url: &str| {
        let mut c = multi_model_config(
            url,
            "test/m1",
            &[
                ("m1", 100_000, &["low", "high"]),
                ("m2", 250_000, &["fast"]),
            ],
        );
        c.agents.insert(
            "specialist".into(),
            AgentConfig {
                name: "specialist".into(),
                description: "Specialist".into(),
                creatable: true,
                model: ModelRef::parse("test/m2"),
                ..Default::default()
            },
        );
        c
    };
    let dir_b_cfg = multi_model_config(&m.url, "test/m2", &[("m2", 320_000, &["fast"])]);
    let live_cfg = Arc::new(Mutex::new(make_rich_cfg(&m.url)));
    let loader_a = live_cfg.clone();
    let loader_b = dir_b_cfg.clone();
    let dir_root = temp_dir("reconcile-cd-resume");
    let dir_a = dir_root.join("dir_a");
    let dir_b = dir_root.join("dir_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let dir_b_canon = dunce::canonicalize(&dir_b).unwrap();
    let dir_b_for_loader = dir_b_canon.clone();
    let mut opts = hub_options(make_rich_cfg(&m.url), &dir_root);
    opts.config_loader = Arc::new(move |cwd| {
        if cwd == dir_b_for_loader {
            loader_b.clone()
        } else {
            loader_a.lock().unwrap().clone()
        }
    });
    let hub = ref_server::Hub::new(opts);
    let conn = hello_conn(&hub);
    let cwd_a = dunce::canonicalize(&dir_a)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    conn.lock().cwd = cwd_a.clone();

    // 1. SetAgent round-trip (!model_explicit): build (m1) -> specialist (m2) -> build restores m1!
    let s = session(&hub, &dir_a, "agent-var-sess", BUILD).await;
    prompt(&hub, &s, "seed message");
    wait_idle(&s).await;
    assert_eq!(s.info().model.key(), "test/m1");
    assert!(!s.lock().model_explicit);

    let _ = call(
        &hub,
        &conn,
        Request::SetAgent {
            session_id: s.id.clone(),
            agent: "specialist".into(),
        },
    )
    .await;
    assert_eq!(s.info().model.key(), "test/m2");
    assert!(!s.lock().model_explicit);

    // Explicit model on specialist, then SetAgent("specialist") again (same agent!) resets & persists!
    let _ = call(
        &hub,
        &conn,
        Request::SetModel {
            session_id: s.id.clone(),
            model: ModelRef::parse("test/m1").unwrap(),
        },
    )
    .await;
    assert!(s.lock().model_explicit);
    let _ = call(
        &hub,
        &conn,
        Request::SetAgent {
            session_id: s.id.clone(),
            agent: "specialist".into(),
        },
    )
    .await;
    assert_eq!(s.info().model.key(), "test/m2");
    assert!(!s.lock().model_explicit);
    hub.store.flush().unwrap();
    let loaded_raw = hub.store.load(&s.id).unwrap();
    assert_eq!(loaded_raw.info.model.key(), "test/m2");
    assert!(!loaded_raw.model_explicit);

    // Switch back to build -> restores global default test/m1!
    let _ = call(
        &hub,
        &conn,
        Request::SetAgent {
            session_id: s.id.clone(),
            agent: BUILD.into(),
        },
    )
    .await;
    assert_eq!(s.info().model.key(), "test/m1");
    assert!(!s.lock().model_explicit);

    // 2. SetVariant persists to SQLite AND valid variant on !model_explicit survives ReloadConfig!
    let _ = call(
        &hub,
        &conn,
        Request::SetVariant {
            session_id: s.id.clone(),
            variant: Some("high".into()),
        },
    )
    .await;
    assert_eq!(s.info().model.variant.as_deref(), Some("high"));
    hub.store.flush().unwrap();
    assert_eq!(
        hub.store.load(&s.id).unwrap().info.model.variant.as_deref(),
        Some("high")
    );

    let _ = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    assert_eq!(s.info().model.variant.as_deref(), Some("high"));
    assert!(!s.lock().model_explicit);

    // 3. Resume reconciliation in ops::get_or_load when custom agent & model were removed while closed.
    let s_resume = session(&hub, &dir_a, "resume-sess", "specialist").await;
    let _ = call(
        &hub,
        &conn,
        Request::SetModel {
            session_id: s_resume.id.clone(),
            model: ModelRef::parse("test/m1#high").unwrap(),
        },
    )
    .await;
    prompt(&hub, &s_resume, "before close");
    wait_idle(&s_resume).await;
    let resume_id = s_resume.id.clone();
    ref_server::ops::close_session(&hub, &s_resume).await;
    hub.store.flush().unwrap();

    // Remove "specialist" and "m1" from dir_a config before resuming.
    *live_cfg.lock().unwrap() =
        multi_model_config(&m.url, "test/m2", &[("m2", 280_000, &["fast"])]);
    let resumed = ref_server::ops::get_or_load(&hub, &resume_id)
        .await
        .unwrap();
    assert_eq!(resumed.info().agent, BUILD);
    assert_eq!(resumed.info().model.key(), "test/m2");
    assert_eq!(resumed.info().model.variant, None);
    assert_eq!(resumed.info().context.max, 280_000);
    assert!(!resumed.lock().model_explicit);
    hub.store.flush().unwrap();
    assert_eq!(hub.store.load(&resume_id).unwrap().info.agent, BUILD);
    prompt(&hub, &resumed, "after resume");
    wait_idle(&resumed).await;
    assert_eq!(resumed.info().last_error, None);

    // 4. SetCwd (/cd) reconciles agent, model, variant, and context against target dir's config!
    *live_cfg.lock().unwrap() = make_rich_cfg(&m.url);
    let s_cd = session(&hub, &dir_a, "cd-sess", "specialist").await;
    prompt(&hub, &s_cd, "in dir_a");
    wait_idle(&s_cd).await;
    assert_eq!(s_cd.info().agent, "specialist");
    assert_eq!(s_cd.info().context.max, 250_000);

    let _ = call(
        &hub,
        &conn,
        Request::SetCwd {
            session_id: s_cd.id.clone(),
            path: dir_b_canon.to_string_lossy().into_owned(),
        },
    )
    .await;
    assert_eq!(s_cd.info().agent, BUILD);
    assert_eq!(s_cd.info().model.key(), "test/m2");
    assert_eq!(s_cd.info().context.max, 320_000);
    hub.store.flush().unwrap();
    assert_eq!(hub.store.load(&s_cd.id).unwrap().info.context.max, 320_000);
}

const GATED_MCP_PY: &str = r#"
import sys, json, os, time

pid_path = os.environ["PID_FILE"]
tmp_path = pid_path + ".tmp"
with open(tmp_path, "w") as f:
    f.write(str(os.getpid()))
os.replace(tmp_path, pid_path)

gate = os.environ.get("GATE_FILE")
if gate:
    while not os.path.exists(gate):
        time.sleep(0.02)

if os.environ.get("HANG_FOREVER") == "1":
    time.sleep(60)
    sys.exit(0)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method")
    req_id = msg.get("id")
    if method == "initialize":
        res = {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "gated", "version": "1.0"},
        }
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": res}) + "\n")
        sys.stdout.flush()
    elif method == "tools/list":
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"tools": []}}) + "\n")
        sys.stdout.flush()
"#;

#[tokio::test]
async fn prepare_skips_stale_system_cache_after_concurrent_reload_or_set_agent() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let dir = temp_dir("prep-race");
    let pid_file = dir.join("gated.pid");
    let gate_file = dir.join("gate.ok");

    let make_gated_cfg = |build_prompt: &str| {
        let mut c = config(&m.url);
        c.agents.get_mut(BUILD).unwrap().prompt = build_prompt.into();
        c.agents.get_mut(WORKER).unwrap().prompt = "WORKER_CUSTOM_PROMPT".into();
        let mut env = BTreeMap::new();
        env.insert("PID_FILE".into(), pid_file.to_string_lossy().into_owned());
        env.insert("GATE_FILE".into(), gate_file.to_string_lossy().into_owned());
        c.mcp.insert(
            "gated".into(),
            McpConfig::Local {
                command: vec![
                    "python3".into(),
                    "-u".into(),
                    "-c".into(),
                    GATED_MCP_PY.into(),
                ],
                environment: env,
                cwd: None,
                enabled: true,
                timeout_ms: Some(30_000),
            },
        );
        c
    };

    let live_cfg = Arc::new(Mutex::new(config(&m.url)));
    let loader_cfg = live_cfg.clone();
    let (hub, work) = hub_with(config(&m.url), "prep-race-hub", |opts| {
        opts.config_loader = Arc::new(move |_| loader_cfg.lock().unwrap().clone());
    });
    let s = session(&hub, &work, "race-sess", BUILD).await;
    {
        let mut st = s.lock();
        st.cfg = Arc::new(make_gated_cfg("INITIAL_BUILD_PROMPT"));
        st.system = None;
    }
    prompt(&hub, &s, "turn 1");
    wait_until("gated mcp spawned inside prepare()", TIMEOUT, || {
        pid_file.exists()
    })
    .await;
    // Reload config while prepare() is awaiting srv.wait_ready(...)!
    *live_cfg.lock().unwrap() = make_gated_cfg("RELOADED_BUILD_PROMPT");
    ref_server::ops::reload_all(&hub, Some(&work.to_string_lossy()))
        .await
        .unwrap();
    std::fs::write(&gate_file, "go").unwrap();
    wait_idle(&s).await;

    let cached_after_reload = s
        .lock()
        .system
        .clone()
        .expect("re-prepared fresh system")
        .join("\n");
    assert!(cached_after_reload.contains("RELOADED_BUILD_PROMPT"));
    assert!(!cached_after_reload.contains("INITIAL_BUILD_PROMPT"));
    assert!(m.bodies()[0].to_string().contains("RELOADED_BUILD_PROMPT"));
    assert!(!m.bodies()[0].to_string().contains("INITIAL_BUILD_PROMPT"));

    // Now verify SetAgent race while prepare() awaits wait_ready (same Arc<Config>, changed agent!).
    let pid_file_2 = dir.join("gated2.pid");
    let gate_file_2 = dir.join("gate2.ok");
    let mut cfg_2 = make_gated_cfg("RELOADED_BUILD_PROMPT");
    {
        let mut env = BTreeMap::new();
        env.insert("PID_FILE".into(), pid_file_2.to_string_lossy().into_owned());
        env.insert(
            "GATE_FILE".into(),
            gate_file_2.to_string_lossy().into_owned(),
        );
        cfg_2.mcp.insert(
            "gated".into(),
            McpConfig::Local {
                command: vec![
                    "python3".into(),
                    "-u".into(),
                    "-c".into(),
                    GATED_MCP_PY.into(),
                ],
                environment: env,
                cwd: None,
                enabled: true,
                timeout_ms: Some(30_000),
            },
        );
        let mut st = s.lock();
        st.cfg = Arc::new(cfg_2);
        st.system = None;
    }
    let conn = hello_conn(&hub);
    prompt(&hub, &s, "turn 2");
    wait_until("gated2 mcp spawned inside prepare()", TIMEOUT, || {
        pid_file_2.exists()
    })
    .await;
    let _ = call(
        &hub,
        &conn,
        Request::SetAgent {
            session_id: s.id.clone(),
            agent: WORKER.into(),
        },
    )
    .await;
    std::fs::write(&gate_file_2, "go").unwrap();
    wait_idle(&s).await;
    let cached_after_switch = s
        .lock()
        .system
        .clone()
        .expect("re-prepared worker system")
        .join("\n");
    assert!(cached_after_switch.contains("WORKER_CUSTOM_PROMPT"));
    assert!(!cached_after_switch.contains("RELOADED_BUILD_PROMPT"));
    assert!(m.bodies()[1].to_string().contains("WORKER_CUSTOM_PROMPT"));
    assert!(!m.bodies()[1].to_string().contains("RELOADED_BUILD_PROMPT"));
    ref_server::ops::shutdown_all(&hub).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reload_all_preserves_new_session_mcp_and_aborts_hung_connecting_mcp() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let dir = temp_dir("reload-mcp-races");
    let work_1 = dir.join("w1");
    let work_2 = dir.join("w2");
    std::fs::create_dir_all(&work_1).unwrap();
    std::fs::create_dir_all(&work_2).unwrap();
    let w1_canon = dunce::canonicalize(&work_1).unwrap();
    let w2_canon = dunce::canonicalize(&work_2).unwrap();
    let keep_pid_file = dir.join("keep.pid");
    let hung_pid_file = dir.join("hung.pid");

    let mut cfg_w2 = config(&m.url);
    {
        let mut env = BTreeMap::new();
        env.insert(
            "PID_FILE".into(),
            keep_pid_file.to_string_lossy().into_owned(),
        );
        cfg_w2.mcp.insert(
            "keep".into(),
            McpConfig::Local {
                command: vec![
                    "python3".into(),
                    "-u".into(),
                    "-c".into(),
                    GATED_MCP_PY.into(),
                ],
                environment: env,
                cwd: None,
                enabled: true,
                timeout_ms: Some(30_000),
            },
        );
    }
    let mut cfg_hung = config(&m.url);
    {
        let mut env = BTreeMap::new();
        env.insert(
            "PID_FILE".into(),
            hung_pid_file.to_string_lossy().into_owned(),
        );
        env.insert("HANG_FOREVER".into(), "1".into());
        cfg_hung.mcp.insert(
            "hung".into(),
            McpConfig::Local {
                command: vec![
                    "python3".into(),
                    "-u".into(),
                    "-c".into(),
                    GATED_MCP_PY.into(),
                ],
                environment: env,
                cwd: None,
                enabled: true,
                timeout_ms: Some(60_000),
            },
        );
    }
    let w1_cfg = Arc::new(Mutex::new(cfg_hung.clone()));
    let entered = Arc::new(AtomicBool::new(false));
    let proceed = Arc::new(AtomicBool::new(false));
    let block_next_w1 = Arc::new(AtomicBool::new(false));
    let w1_for_loader = w1_canon.clone();
    let w2_for_loader = w2_canon.clone();
    let w1_cfg_loader = w1_cfg.clone();
    let cfg_w2_loader = cfg_w2.clone();
    let entered_l = entered.clone();
    let proceed_l = proceed.clone();
    let block_l = block_next_w1.clone();
    let mut opts = hub_options(config(&m.url), &dir);
    opts.config_loader = Arc::new(move |cwd| {
        if cwd == w2_for_loader {
            return cfg_w2_loader.clone();
        }
        if cwd == w1_for_loader && block_l.swap(false, Ordering::SeqCst) {
            entered_l.store(true, Ordering::SeqCst);
            while !proceed_l.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        w1_cfg_loader.lock().unwrap().clone()
    });
    let hub = ref_server::Hub::new(opts);
    let conn = hello_conn(&hub);
    let w1_str = w1_canon.to_string_lossy().into_owned();
    conn.lock().cwd = w1_str.clone();

    // 1. Start hung MCP server in Connecting state (60s timeout).
    let _ = call(
        &hub,
        &conn,
        Request::McpStatus {
            cwd: Some(w1_str.clone()),
        },
    )
    .await;
    wait_until("hung mcp spawned", TIMEOUT, || hung_pid_file.exists()).await;
    let hung_pid: u32 = std::fs::read_to_string(&hung_pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(ref_sys::pid_alive(hung_pid));

    // 2. Disable hung server in w1, block reload_all inside load_config(w1), and create session in w2!
    *w1_cfg.lock().unwrap() = config(&m.url);
    block_next_w1.store(true, Ordering::SeqCst);
    let hub_reload = hub.clone();
    let w1_trigger = w1_str.clone();
    let reload_task =
        tokio::spawn(
            async move { ref_server::ops::reload_all(&hub_reload, Some(&w1_trigger)).await },
        );
    wait_until("reload_all inside load_config", TIMEOUT, || {
        entered.load(Ordering::SeqCst)
    })
    .await;

    let _s2 = session(&hub, &work_2, "new-w2-sess", BUILD).await;
    wait_until("keep mcp connected", TIMEOUT, || {
        keep_pid_file.exists()
            && hub
                .mcp
                .status(&cfg_w2)
                .iter()
                .any(|s| s.name == "keep" && s.status == ref_proto::MCP_CONNECTED)
    })
    .await;
    let keep_pid: u32 = std::fs::read_to_string(&keep_pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(ref_sys::pid_alive(keep_pid));

    proceed.store(true, Ordering::SeqCst);
    reload_task.await.unwrap().unwrap();

    // Hung Connecting child was aborted & killed immediately (not waiting 60s), while s2's MCP stayed alive!
    wait_until(
        "hung connecting mcp killed immediately",
        std::time::Duration::from_secs(5),
        || !ref_sys::pid_alive(hung_pid),
    )
    .await;
    assert!(
        ref_sys::pid_alive(keep_pid),
        "newly created session's MCP must not be evicted"
    );
    assert!(
        hub.mcp
            .status(&cfg_w2)
            .iter()
            .any(|s| { s.name == "keep" && s.status == ref_proto::MCP_CONNECTED })
    );
    ref_server::ops::shutdown_all(&hub).await;
    wait_until("keep mcp closed on shutdown", TIMEOUT, || {
        !ref_sys::pid_alive(keep_pid)
    })
    .await;
}
