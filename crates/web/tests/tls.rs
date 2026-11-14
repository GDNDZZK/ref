//! TLS mode (`--host 0.0.0.0`, `--no-tls`, certificate management, 308 redirect, pinned probe).
//! No test here depends on a machine's primary outbound IP in the SAN.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Error as WsError;

const NO_BACKEND: &str = "ws://127.0.0.1:1/";

fn read_record(env: &Env, pid: u32) -> serde_json::Value {
    let path = env.state_dir().join(format!("web/instances/{pid}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn cert_fingerprint(path: &Path) -> String {
    let der = tls_pin::load_cert_der(path).unwrap();
    tls_pin::cert_fingerprint(der.as_ref())
}

async fn connect_pinned_tls(port: u16, cert_path: &Path) -> Result<String, std::io::Error> {
    let der = tls_pin::load_cert_der(cert_path).unwrap();
    let connector = TlsConnector::from(Arc::new(tls_pin::pinned_client_config(der)));
    let tcp = TcpStream::connect((LOOPBACK, port)).await?;
    let server_name = ServerName::try_from("localhost").unwrap();
    let stream = connector.connect(server_name, tcp).await?;
    let leaf = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first())
        .expect("server sent leaf certificate");
    Ok(tls_pin::cert_fingerprint(leaf.as_ref()))
}

#[tokio::test]
async fn https_and_wss_work_and_printed_fingerprint_matches_served_leaf() {
    let env = Env::new("tls-basic");
    let upstream = fake_upstream().await;
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let web = env.start(&["--host", "0.0.0.0", "--server", &server]).await;
    assert!(web.tls);
    assert_eq!(read_record(&env, web.pid())["tls"], true);

    assert_eq!(web.get("/").await.status(), 200);
    assert_eq!(web.get("/api/ping?nonce=00ff").await.status(), 200);

    let mut channel = web.channel().await;
    channel.send_text("hello over tls").await;
    assert_eq!(channel.next_text().await, "hello over tls");

    // Plain http:// Origin is refused on a TLS server.
    let bad_origin = format!("http://{LOOPBACK}:{}", web.port);
    match web.try_ws(Some(bad_origin), None).await {
        Err(WsError::Http(resp)) => assert_eq!(resp.status(), 403),
        other => panic!("expected 403 for http Origin on TLS server, got {other:?}"),
    }

    // Served leaf matches cert.pem and the printed fingerprint.
    let disk_fp = cert_fingerprint(&web.cert_path());
    let served_fp = connect_pinned_tls(web.port, &web.cert_path())
        .await
        .unwrap();
    assert_eq!(served_fp, disk_fp);

    let stdout = web.stdout();
    let stderr = web.stderr();
    assert!(
        stdout.contains(&format!("ref-web: certificate SHA-256 {disk_fp}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("https://{LOOPBACK}:{}", web.port)),
        "{stdout}"
    );
    assert!(stderr.contains(&disk_fp), "{stderr}");
    assert!(stderr.contains("Check the fingerprint"), "{stderr}");

    // A pinned connector expecting a different certificate refuses the connection.
    let wrong_der = rustls::pki_types::CertificateDer::from(vec![1, 2, 3, 4]);
    let wrong_connector = TlsConnector::from(Arc::new(tls_pin::pinned_client_config(wrong_der)));
    let tcp = TcpStream::connect((LOOPBACK, web.port)).await.unwrap();
    let err = wrong_connector
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[cfg(unix)]
#[tokio::test]
async fn key_pem_is_private_and_certificate_is_reused_across_restart() {
    use std::os::unix::fs::PermissionsExt as _;

    let env = Env::new("tls-reuse");
    let mut first = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;
    let tls_dir = env.state_dir().join("web/tls");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&tls_dir), 0o700);
    assert_eq!(mode(&tls_dir.join("key.pem")), 0o600);

    let first_fp = cert_fingerprint(&first.cert_path());
    assert!(ref_sys::terminate(first.pid()));
    assert!(first.wait_exit().await.success());

    let second = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;
    assert_eq!(cert_fingerprint(&second.cert_path()), first_fp);
    assert!(
        !second.stdout().contains("certificate renewed"),
        "{}",
        second.stdout()
    );
}

#[tokio::test]
async fn missing_name_in_meta_regenerates_certificate() {
    let env = Env::new("tls-regen");
    let mut first = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;
    let first_fp = cert_fingerprint(&first.cert_path());
    assert!(ref_sys::terminate(first.pid()));
    assert!(first.wait_exit().await.success());

    // Drop `::1` from meta.json so the next start sees a missing SAN and regenerates.
    let meta_path = env.state_dir().join("web/tls/meta.json");
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    let names = meta["names"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v.as_str() != Some("::1"))
        .cloned()
        .collect::<Vec<_>>();
    meta["names"] = serde_json::Value::Array(names);
    std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();

    let second = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;
    let second_fp = cert_fingerprint(&second.cert_path());
    assert_ne!(second_fp, first_fp);
    let stdout = second.stdout();
    assert!(
        stdout.contains(&format!(
            "ref-web: certificate renewed; new SHA-256 {second_fp}"
        )),
        "{stdout}"
    );
    let updated_meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert!(
        updated_meta["names"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("::1"))
    );
}

#[tokio::test]
async fn http_on_tls_port_gets_308_without_query() {
    let env = Env::new("tls-redirect");
    let web = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;

    let resp = web
        .http
        .get(format!(
            "http://{LOOPBACK}:{}/login?token=secret&dir=/tmp",
            web.port
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 308);
    assert_eq!(
        resp.headers()["location"],
        format!("https://{LOOPBACK}:{}/", web.port)
    );

    // Disallowed Host on plain HTTP still gets 403.
    let mut stream = TcpStream::connect((LOOPBACK, web.port)).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: evil.example:8443\r\n\r\n")
        .await
        .unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(STEP_TIMEOUT, stream.read_to_end(&mut raw))
        .await
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(text.starts_with("HTTP/1.1 403 "), "{text}");
}

#[tokio::test]
async fn no_tls_on_non_loopback_serves_plain_http() {
    let env = Env::new("no-tls-lan");
    let upstream = fake_upstream().await;
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let web = env
        .start(&["--host", "0.0.0.0", "--no-tls", "--server", &server])
        .await;
    assert!(!web.tls);
    assert_eq!(read_record(&env, web.pid())["tls"], false);
    assert!(!env.state_dir().join("web/tls").exists());
    assert_eq!(web.get("/").await.status(), 200);

    let mut channel = web.channel().await;
    channel.send_text("plain").await;
    assert_eq!(channel.next_text().await, "plain");

    let stderr = web.stderr();
    assert!(stderr.contains("without TLS"), "{stderr}");
}

#[tokio::test]
async fn no_tls_on_loopback_starts_silently() {
    let env = Env::new("no-tls-loopback");
    let web = env.start(&["--no-tls", "--server", NO_BACKEND]).await;
    assert!(!web.tls);
    assert!(!env.state_dir().join("web/tls").exists());
    assert!(web.stderr().is_empty(), "{}", web.stderr());
    assert!(web.stdout().contains("http://127.0.0.1:"));
}

#[tokio::test]
async fn second_start_finds_tls_instance_and_refuses_different_key() {
    let env = Env::new("tls-second");
    let first = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;
    let fp = cert_fingerprint(&first.cert_path());

    let second = env.run(&["--no-open", "--server", NO_BACKEND]);
    assert!(second.status.success(), "{second:?}");
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(
        out.contains(&format!("ref-web: certificate SHA-256 {fp}")),
        "{out}"
    );
    assert!(
        out.contains(&format!("already running on port {}", first.port)),
        "{out}"
    );
    assert!(
        out.contains(&format!("https://{LOOPBACK}:{}/", first.port)),
        "{out}"
    );
    let records = std::fs::read_dir(env.state_dir().join("web/instances"))
        .unwrap()
        .count();
    assert_eq!(records, 1);

    let wrong = env.run(&[
        "--no-open",
        "--port",
        &first.port.to_string(),
        "--key",
        "a different key of enough length",
    ]);
    assert!(!wrong.status.success());
    let err = String::from_utf8_lossy(&wrong.stderr);
    assert!(
        err.contains(&format!(
            "a ref-web with a different key is running on port {} (pid {})",
            first.port,
            first.pid()
        )),
        "{err}"
    );
}
