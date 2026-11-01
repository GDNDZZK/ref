//! Port choice, instance records and finding a running instance.

mod common;

use common::*;

/// A backend address nothing listens on; these tests never open a tab.
const NO_BACKEND: &str = "ws://127.0.0.1:1/";
const RANDOM_PORTS: std::ops::RangeInclusive<u16> = 10000..=65535;

fn record_path(env: &Env, pid: u32) -> std::path::PathBuf {
    env.state_dir().join(format!("web/instances/{pid}.json"))
}

fn read_record(env: &Env, pid: u32) -> serde_json::Value {
    let text = std::fs::read_to_string(record_path(env, pid)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn without_port_ref_web_picks_a_random_port_and_records_it() {
    let env = Env::new("random-port");
    let web = env.start(&["--server", NO_BACKEND]).await;
    assert!(RANDOM_PORTS.contains(&web.port), "{}", web.port);

    let record = read_record(&env, web.pid());
    assert_eq!(record["pid"], web.pid());
    assert_eq!(record["port"], web.port);
    assert_eq!(record["host"], "127.0.0.1");
    assert_eq!(record["tls"], false);
    assert_eq!(record["version"], env!("CARGO_PKG_VERSION"));
    assert!(record["started_at"].as_i64().unwrap() > 0);
}

#[cfg(unix)]
#[tokio::test]
async fn the_record_is_private_and_goes_away_on_a_graceful_exit() {
    use std::os::unix::fs::PermissionsExt as _;
    let env = Env::new("record-life");
    let mut web = env.start(&["--server", NO_BACKEND]).await;
    let path = record_path(&env, web.pid());
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);

    assert!(ref_sys::terminate(web.pid()));
    assert!(web.wait_exit().await.success());
    assert!(!path.exists(), "removed on exit");
}

#[tokio::test]
async fn a_restart_gets_the_same_port() {
    let env = Env::new("same-port");
    let mut first = env.start(&["--server", NO_BACKEND]).await;
    let port = first.port;
    assert!(ref_sys::terminate(first.pid()));
    first.wait_exit().await;

    let second = env.start(&["--server", NO_BACKEND]).await;
    assert_eq!(second.port, port);
}

#[tokio::test]
async fn a_killed_instance_leaves_a_stale_record_that_the_next_start_removes() {
    let env = Env::new("stale");
    let mut first = env.start(&["--server", NO_BACKEND]).await;
    let stale = record_path(&env, first.pid());
    first.kill_hard();
    assert!(stale.exists(), "SIGKILL cannot clean up");

    let second = env.start(&["--server", NO_BACKEND]).await;
    assert!(!stale.exists(), "removed by the next start");
    assert!(record_path(&env, second.pid()).exists());
}

#[tokio::test]
async fn a_second_start_without_port_opens_the_running_instance() {
    let env = Env::new("find-running");
    let web = env.start(&["--server", NO_BACKEND]).await;

    let second = env.run(&["--no-open", "--server", NO_BACKEND]);
    assert!(second.status.success(), "{second:?}");
    let out = stdout_of(&second);
    assert!(
        out.contains(&format!("already running on port {}", web.port)),
        "{out}"
    );
    let records = std::fs::read_dir(env.state_dir().join("web/instances"))
        .unwrap()
        .count();
    assert_eq!(records, 1, "no second instance was started");
}

#[tokio::test]
async fn options_of_a_second_start_are_reported_as_ignored() {
    let env = Env::new("ignored-options");
    let _web = env.start(&["--server", NO_BACKEND]).await;

    let second = env.run(&["--no-open", "--mock"]);
    assert!(second.status.success(), "{second:?}");
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(
        err.contains("keeps its own --server, --mock and --host"),
        "{err}"
    );
}

#[tokio::test]
async fn a_second_start_with_another_port_starts_its_own_instance() {
    let env = Env::new("other-port");
    let first = env.start(&["--server", NO_BACKEND]).await;
    let free = free_port();
    assert_ne!(free, first.port);

    let second = env.start_on(free, &["--server", NO_BACKEND]).await;
    assert_eq!(second.port, free);
    assert_ne!(second.pid(), first.pid());
}
