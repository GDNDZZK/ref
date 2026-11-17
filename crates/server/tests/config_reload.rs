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
    let proj_override = work.join(".ref/ref.json");
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

#[tokio::test]
async fn mcp_reconcile_retries_failed_servers_and_closes_removed_ones() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let dir = temp_dir("mcp-reconcile");
    let flag_file = dir.join("allow-mcp");
    let pid_file = dir.join("mcp.pid");
    let script = dir.join("fake_mcp.sh");

    // Minimal stdio MCP server: fails until flag_file exists; once started, writes its PID and answers initialize + tools/list.
    let script_body = format!(
        "#!/bin/sh\nif [ ! -f \"{}\" ]; then\n  echo 'not ready yet' >&2\n  exit 1\nfi\necho $$ > \"{}\"\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *\"\\\"initialize\\\"\"*)\n      printf '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"serverInfo\":{{\"name\":\"fake\",\"version\":\"1.0\"}}}}}}\\n'\n      ;;\n    *\"\\\"tools/list\\\"\"*)\n      printf '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"tools\":[{{\"name\":\"ping\",\"description\":\"d\",\"inputSchema\":{{\"type\":\"object\"}}}}]}}}}\\n'\n      ;;\n  esac\ndone\n",
        flag_file.display(),
        pid_file.display()
    );
    std::fs::write(&script, script_body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let make_cfg = |enabled: bool| {
        let mut c = config(&m.url);
        c.mcp.insert(
            "fake".into(),
            McpConfig::Local {
                command: vec!["/bin/sh".into(), script.to_string_lossy().into_owned()],
                environment: BTreeMap::new(),
                cwd: None,
                enabled,
                timeout_ms: Some(5_000),
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

    // Disable server and reload -> server is removed and child exits on stdin EOF.
    *live_cfg.lock().unwrap() = make_cfg(false);
    let _ = call(&hub, &conn, Request::ReloadConfig { cwd: None }).await;
    let status_after = call(&hub, &conn, Request::McpStatus { cwd: None }).await;
    assert_eq!(
        status_after["servers"][0]["status"],
        ref_proto::MCP_DISABLED
    );
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
