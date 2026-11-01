//! Choosing the port and binding the listener.
//!
//! Without `--port`, ref-web picks a random free port in `RANDOM_PORTS`. The last port it
//! used is kept in `<state dir>/web/port` and tried first, so the address (and with it the
//! browser's origin, settings and certificate exception) survives restarts.

use std::ops::RangeInclusive;
use std::path::Path;

use anyhow::Context as _;
use rand::Rng as _;
use tokio::net::TcpListener;

use crate::launch::bare_host;
use crate::token;

const RANDOM_PORTS: RangeInclusive<u16> = 10000..=65535;
/// Random ports tried before giving up (each is a single bind, so this is quick).
const MAX_BIND_TRIES: u32 = 500;
const LAST_PORT_FILE: &str = "port";

/// Binds `host:port` for `--port`, or a free random port without it.
pub async fn bind(host: &str, requested: Option<u16>) -> anyhow::Result<TcpListener> {
    bind_in(&token::state_subdir(), host, requested).await
}

async fn bind_in(
    state_dir: &Path,
    host: &str,
    requested: Option<u16>,
) -> anyhow::Result<TcpListener> {
    let host = bare_host(host);
    if let Some(port) = requested {
        return bind_requested(host, port).await;
    }
    let last = read_last_port(state_dir);
    if let Some(port) = last
        && let Ok(listener) = TcpListener::bind((host, port)).await
    {
        return Ok(listener);
    }
    let listener = bind_random(host).await?;
    let port = listener.local_addr()?.port();
    if last != Some(port)
        && let Err(e) = write_last_port(state_dir, port)
    {
        tracing::warn!("cannot remember the port: {e}");
    }
    Ok(listener)
}

async fn bind_requested(host: &str, port: u16) -> anyhow::Result<TcpListener> {
    match TcpListener::bind((host, port)).await {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => anyhow::bail!(
            "port {port} is in use by another program or by a ref-web with a different login \
             token (choose another port with --port, or leave --port out)"
        ),
        Err(e) => Err(e).with_context(|| format!("cannot listen on {host}:{port}")),
    }
}

/// Binds the real listener at each try, so no other program can take the port between
/// "free" and "bound".
async fn bind_random(host: &str) -> anyhow::Result<TcpListener> {
    for _ in 0..MAX_BIND_TRIES {
        let port = rand::rng().random_range(RANDOM_PORTS);
        match TcpListener::bind((host, port)).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => return Err(e).with_context(|| format!("cannot listen on {host}:{port}")),
        }
    }
    anyhow::bail!(
        "no free port in {}-{} after {MAX_BIND_TRIES} tries",
        RANDOM_PORTS.start(),
        RANDOM_PORTS.end()
    )
}

fn read_last_port(state_dir: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(state_dir.join(LAST_PORT_FILE)).ok()?;
    let port: u16 = text.trim().parse().ok()?;
    RANDOM_PORTS.contains(&port).then_some(port)
}

fn write_last_port(state_dir: &Path, port: u16) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    std::fs::write(state_dir.join(LAST_PORT_FILE), port.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOPBACK: &str = "127.0.0.1";

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ref-web-listen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn port_of(listener: &TcpListener) -> u16 {
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn a_random_port_is_in_range_and_remembered() {
        let dir = temp_dir("random");
        let listener = bind_in(&dir, LOOPBACK, None).await.unwrap();
        let port = port_of(&listener);
        assert!(RANDOM_PORTS.contains(&port), "{port}");
        assert_eq!(read_last_port(&dir), Some(port));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_remembered_port_is_reused_when_free() {
        let dir = temp_dir("reuse");
        let first = port_of(&bind_in(&dir, LOOPBACK, None).await.unwrap());
        // The first listener is gone, so its port is free again.
        let second = port_of(&bind_in(&dir, LOOPBACK, None).await.unwrap());
        assert_eq!(first, second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_taken_remembered_port_gives_a_new_one_and_updates_the_file() {
        let dir = temp_dir("taken");
        let holder = bind_in(&dir, LOOPBACK, None).await.unwrap();
        let held = port_of(&holder);
        let other = bind_in(&dir, LOOPBACK, None).await.unwrap();
        assert_ne!(port_of(&other), held);
        assert_eq!(read_last_port(&dir), Some(port_of(&other)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_requested_port_is_used_but_not_remembered() {
        let dir = temp_dir("requested");
        let free = port_of(&TcpListener::bind((LOOPBACK, 0)).await.unwrap());
        let listener = bind_in(&dir, LOOPBACK, Some(free)).await.unwrap();
        assert_eq!(port_of(&listener), free);
        assert_eq!(read_last_port(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_requested_port_that_is_taken_is_an_error() {
        let dir = temp_dir("busy");
        let holder = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
        let error = bind_in(&dir, LOOPBACK, Some(port_of(&holder)))
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("in use by another program"), "{error}");
    }

    #[test]
    fn only_ports_in_the_random_range_are_remembered() {
        let dir = temp_dir("range");
        std::fs::create_dir_all(&dir).unwrap();
        for (text, expected) in [
            ("12345", Some(12345)),
            ("80", None),
            ("junk", None),
            ("", None),
        ] {
            std::fs::write(dir.join(LAST_PORT_FILE), text).unwrap();
            assert_eq!(read_last_port(&dir), expected, "{text:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
