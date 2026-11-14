//! Self-signed TLS certificate management and `axum::serve::Listener` (`DESIGN-0.2.1.md` §7.6).
//!
//! Plain `http://` connections arriving on the TLS port receive a `308 Permanent Redirect` to
//! `https://<Host>/` (dropping path and query so no `?token=` is echoed back).

use std::collections::HashSet;
use std::io::Write as _;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use axum::serve::Listener;
use rcgen::{CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair};
use rustls::ServerConfig;
use rustls::crypto::aws_lc_rs::default_provider;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::guard::{HostPolicy, LOCALHOST};
use crate::launch;
use crate::tls_pin::cert_fingerprint;
use crate::token;

const TLS_DIR: &str = "tls";
const CERT_FILE: &str = "cert.pem";
const KEY_FILE: &str = "key.pem";
const META_FILE: &str = "meta.json";
const CERT_CN: &str = "ref-web";
const CERT_VALIDITY_DAYS: i64 = 3650;
const CLOCK_SKEW_MARGIN_HOURS: i64 = 1;
const MS_PER_SECOND: i64 = 1000;
const RENEW_WITHIN_MS: i64 = 30 * 24 * 60 * 60 * MS_PER_SECOND;
const ALPN_HTTP_1_1: &[u8] = b"http/1.1";

const LOOPBACK_NAMES: [&str; 3] = ["localhost", "127.0.0.1", "::1"];
/// Unrouted TEST-NET-1 destination used only with `UdpSocket::connect` to query the primary
/// outbound IPv4 address without sending a packet.
const ROUTE_PROBE_ADDR: (&str, u16) = ("192.0.2.1", 9);

const TLS_HANDSHAKE_RECORD: u8 = 0x16;
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TLS_HANDSHAKES: usize = 64;
const MAX_HTTP_HEAD_BYTES: usize = 8192;
const HTTP_READ_CHUNK: usize = 1024;

#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const GROUP_OTHER_BITS: u32 = 0o077;
#[cfg(unix)]
const PERMISSION_BITS: u32 = 0o777;

pub struct LoadedTls {
    pub acceptor: TlsAcceptor,
    pub fingerprint: String,
    pub renewed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CertMeta {
    names: Vec<String>,
    not_after: i64,
}

pub fn tls_dir() -> anyhow::Result<PathBuf> {
    Ok(token::state_subdir()?.join(TLS_DIR))
}

pub fn cert_path() -> anyhow::Result<PathBuf> {
    Ok(tls_dir()?.join(CERT_FILE))
}

pub fn is_loopback(host: &str) -> bool {
    let bare = launch::bare_host(host);
    bare.eq_ignore_ascii_case(LOCALHOST) || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

pub fn load_or_create(listen_host: &str) -> anyhow::Result<LoadedTls> {
    let needed = needed_names(listen_host, primary_outbound_ipv4());
    load_or_create_in(&tls_dir()?, &needed, ref_proto::now_ms())
}

fn load_or_create_in(dir: &Path, needed: &[String], now_ms: i64) -> anyhow::Result<LoadedTls> {
    create_private_dir(dir)?;
    let cert_file = dir.join(CERT_FILE);
    let key_file = dir.join(KEY_FILE);
    let meta_file = dir.join(META_FILE);

    let old_meta = read_meta(&meta_file);
    if let Some(meta) = &old_meta
        && !needs_renewal(meta, needed, now_ms)
        && let Ok(loaded) = load_existing(&cert_file, &key_file)
    {
        return Ok(loaded);
    }

    let had_cert = cert_file.exists();
    let names = match old_meta {
        Some(meta) => union_names(&meta.names, needed),
        None => needed.to_vec(),
    };
    generate_and_store(dir, &names, had_cert)
}

fn read_meta(path: &Path) -> Option<CertMeta> {
    let raw = std::fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn needs_renewal(meta: &CertMeta, needed: &[String], now_ms: i64) -> bool {
    if meta.not_after <= now_ms + RENEW_WITHIN_MS {
        return true;
    }
    needed.iter().any(|name| {
        !meta
            .names
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
    })
}

fn load_existing(cert_file: &Path, key_file: &Path) -> anyhow::Result<LoadedTls> {
    restrict_key_permissions(key_file)?;
    let cert_der = CertificateDer::from_pem_file(cert_file)
        .with_context(|| format!("cannot read {}", cert_file.display()))?;
    let key_der = PrivateKeyDer::from_pem_file(key_file)
        .with_context(|| format!("cannot read {}", key_file.display()))?;
    let fingerprint = cert_fingerprint(cert_der.as_ref());
    let acceptor = build_acceptor(cert_der, key_der)?;
    Ok(LoadedTls {
        acceptor,
        fingerprint,
        renewed: false,
    })
}

fn generate_and_store(dir: &Path, names: &[String], renewed: bool) -> anyhow::Result<LoadedTls> {
    let mut params = CertificateParams::new(names.to_vec()).context("invalid certificate SAN")?;
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CommonName, CERT_CN);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(CLOCK_SKEW_MARGIN_HOURS);
    params.not_after = now + time::Duration::days(CERT_VALIDITY_DAYS);
    let not_after_ms = params.not_after.unix_timestamp() * MS_PER_SECOND;

    let key_pair =
        KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).context("cannot generate TLS key")?;
    let cert = params
        .self_signed(&key_pair)
        .context("cannot sign TLS certificate")?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
        .context("cannot parse generated private key")?;
    let fingerprint = cert_fingerprint(cert_der.as_ref());
    let acceptor = build_acceptor(cert_der, key_der)?;

    let meta = CertMeta {
        names: names.to_vec(),
        not_after: not_after_ms,
    };
    let meta_json = serde_json::to_vec_pretty(&meta).expect("CertMeta serializes");

    write_atomic(&dir.join(KEY_FILE), key_pem.as_bytes(), true)?;
    write_atomic(&dir.join(CERT_FILE), cert_pem.as_bytes(), false)?;
    write_atomic(&dir.join(META_FILE), &meta_json, false)?;

    Ok(LoadedTls {
        acceptor,
        fingerprint,
        renewed,
    })
}

fn build_acceptor(
    cert_der: CertificateDer<'static>,
    key_der: PrivateKeyDer<'static>,
) -> anyhow::Result<TlsAcceptor> {
    let mut config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .context("TLS protocol versions")?
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("invalid TLS certificate or key")?;
    config.alpn_protocols = vec![ALPN_HTTP_1_1.to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn needed_names(listen_host: &str, outbound_ip: Option<IpAddr>) -> Vec<String> {
    let mut names: Vec<String> = LOOPBACK_NAMES.iter().map(|s| (*s).to_string()).collect();
    let bare = launch::bare_host(listen_host);
    match bare.parse::<IpAddr>() {
        Ok(ip) if ip.is_unspecified() => {
            if let Some(ip) = outbound_ip {
                push_unique(&mut names, ip.to_string());
            }
        }
        Ok(ip) => push_unique(&mut names, ip.to_string()),
        Err(_) if !bare.is_empty() => push_unique(&mut names, bare.to_string()),
        Err(_) => {}
    }
    names
}

fn union_names(existing: &[String], needed: &[String]) -> Vec<String> {
    let mut out = existing.to_vec();
    for name in needed {
        push_unique(&mut out, name.clone());
    }
    out
}

fn push_unique(names: &mut Vec<String>, candidate: String) {
    let seen: HashSet<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    if !seen.contains(&candidate.to_ascii_lowercase()) {
        names.push(candidate);
    }
}

fn primary_outbound_ipv4() -> Option<IpAddr> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(ROUTE_PROBE_ADDR).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

fn create_private_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(PRIVATE_DIR_MODE))
            .with_context(|| format!("cannot restrict {}", dir.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_key_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?
        .permissions()
        .mode();
    if mode & GROUP_OTHER_BITS != 0 {
        say_err!(
            "warning: {} was readable by other users (mode {:o}); changed to {PRIVATE_FILE_MODE:o}",
            path.display(),
            mode & PERMISSION_BITS
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .with_context(|| format!("cannot restrict {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_key_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8], private: bool) -> anyhow::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", token::random_hex()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(PRIVATE_FILE_MODE);
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut file = options.open(&tmp)?;
        file.write_all(data)?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot write {}", path.display()));
    }
    Ok(())
}

/// Accepts TCP connections, completes TLS handshakes in spawned tasks (bounded and timed out),
/// and redirects plain HTTP requests to `https://<Host>/`.
pub struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    hosts: HostPolicy,
    handshakes: Arc<Semaphore>,
    ready_tx: mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
    ready_rx: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
}

impl TlsListener {
    pub fn new(tcp: TcpListener, acceptor: TlsAcceptor, hosts: HostPolicy) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel(MAX_TLS_HANDSHAKES);
        TlsListener {
            tcp,
            acceptor,
            hosts,
            handshakes: Arc::new(Semaphore::new(MAX_TLS_HANDSHAKES)),
            ready_tx,
            ready_rx,
        }
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            tokio::select! {
                biased;
                accepted = self.ready_rx.recv() => {
                    return accepted.expect("TlsListener holds ready_tx");
                }
                res = self.tcp.accept() => {
                    match res {
                        Ok((stream, addr)) => {
                            let Ok(permit) = self.handshakes.clone().try_acquire_owned() else {
                                tracing::debug!("too many TLS handshakes in flight; dropping {addr}");
                                continue;
                            };
                            let acceptor = self.acceptor.clone();
                            let hosts = self.hosts.clone();
                            let ready_tx = self.ready_tx.clone();
                            tokio::spawn(async move {
                                let _ = tokio::time::timeout(
                                    TLS_HANDSHAKE_TIMEOUT,
                                    handle_connection(stream, addr, acceptor, hosts, ready_tx, permit),
                                ).await;
                            });
                        }
                        Err(e) => {
                            if !is_connection_error(&e) {
                                tracing::error!("accept error: {e}");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

fn is_connection_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
    )
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    acceptor: TlsAcceptor,
    hosts: HostPolicy,
    ready_tx: mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
    _permit: OwnedSemaphorePermit,
) {
    let mut first = [0u8; 1];
    match stream.peek(&mut first).await {
        Ok(1) => {}
        Ok(_) | Err(_) => return,
    }
    if first[0] == TLS_HANDSHAKE_RECORD {
        match acceptor.accept(stream).await {
            Ok(tls_stream) => {
                // Listener dropped on shutdown; nothing left to accept the stream.
                let _ = ready_tx.send((tls_stream, addr)).await;
            }
            Err(e) => tracing::debug!("TLS handshake failed from {addr}: {e}"),
        }
    } else {
        respond_http_redirect(stream, &hosts).await;
    }
}

async fn respond_http_redirect(mut stream: TcpStream, hosts: &HostPolicy) {
    let mut buf = Vec::with_capacity(HTTP_READ_CHUNK);
    let mut chunk = [0u8; HTTP_READ_CHUNK];
    while buf.len() < MAX_HTTP_HEAD_BYTES {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.windows(2).any(|w| w == b"\n\n")
                {
                    break;
                }
            }
        }
    }
    let response = http_redirect_response(&buf, hosts);
    // Best-effort redirect reply on a plain-HTTP probe before closing the socket.
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Builds a `308 Permanent Redirect` to `https://<Host>/` (dropping path and query) when `Host`
/// is permitted by `hosts`, or `403 Forbidden` otherwise.
fn http_redirect_response(head: &[u8], hosts: &HostPolicy) -> String {
    let text = String::from_utf8_lossy(head);
    let host = text.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("host")
            .then(|| value.trim())
    });
    match host {
        Some(host) if hosts.allows(host) => format!(
            "HTTP/1.1 308 Permanent Redirect\r\n\
             Location: https://{host}/\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\r\n"
        ),
        _ => {
            const BODY: &str = "Forbidden: host not allowed\n";
            format!(
                "HTTP/1.1 403 Forbidden\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n\
                 {BODY}",
                BODY.len()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ref-web-tls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn loopback_detection() {
        for yes in [
            "127.0.0.1",
            "127.0.0.2",
            "127.255.255.254",
            "::1",
            "[::1]",
            "localhost",
            "LOCALHOST",
        ] {
            assert!(is_loopback(yes), "{yes}");
        }
        for no in ["0.0.0.0", "::", "[::]", "192.168.1.20", "mybox", ""] {
            assert!(!is_loopback(no), "{no}");
        }
    }

    #[test]
    fn san_set_rules() {
        let ip_v4: IpAddr = "192.168.1.20".parse().unwrap();
        assert_eq!(
            needed_names("0.0.0.0", Some(ip_v4)),
            vec!["localhost", "127.0.0.1", "::1", "192.168.1.20"]
        );
        assert_eq!(
            needed_names("::", Some(ip_v4)),
            vec!["localhost", "127.0.0.1", "::1", "192.168.1.20"]
        );
        assert_eq!(
            needed_names("0.0.0.0", None),
            vec!["localhost", "127.0.0.1", "::1"]
        );
        assert_eq!(
            needed_names("10.0.0.5", Some(ip_v4)),
            vec!["localhost", "127.0.0.1", "::1", "10.0.0.5"]
        );
        assert_eq!(
            needed_names("mybox", None),
            vec!["localhost", "127.0.0.1", "::1", "mybox"]
        );
        // Loopback values do not duplicate the always-present entries.
        assert_eq!(
            needed_names("127.0.0.1", Some(ip_v4)),
            vec!["localhost", "127.0.0.1", "::1"]
        );
        assert_eq!(
            needed_names("LOCALHOST", None),
            vec!["localhost", "127.0.0.1", "::1"]
        );
    }

    #[test]
    fn fingerprint_format_is_uppercase_colon_hex() {
        let fp = cert_fingerprint(b"hello");
        // SHA-256("hello") = 2CF24DBA...
        assert!(fp.starts_with("2C:F2:4D:BA:"), "{fp}");
        assert_eq!(fp.len(), 32 * 2 + 31);
        assert!(
            fp.split(':')
                .all(|pair| pair.len() == 2 && pair.bytes().all(|b| b.is_ascii_hexdigit()))
        );
    }

    #[test]
    fn certificate_reused_until_expiry_or_missing_name() {
        let dir = temp_dir("reuse");
        let now = 1_700_000_000_000i64;
        let names = needed_names("10.0.0.5", None);
        let first = load_or_create_in(&dir, &names, now).unwrap();
        assert!(!first.renewed);

        // Same names, plenty of validity left -> same cert & fingerprint.
        let second = load_or_create_in(&dir, &names, now + 1000).unwrap();
        assert!(!second.renewed);
        assert_eq!(second.fingerprint, first.fingerprint);

        // New name -> regenerates with union of old and new names.
        let other_names = needed_names("10.0.0.6", None);
        let third = load_or_create_in(&dir, &other_names, now + 2000).unwrap();
        assert!(third.renewed);
        assert_ne!(third.fingerprint, first.fingerprint);

        // Switching back to the previous name -> still covered by the union, no regeneration!
        let fourth = load_or_create_in(&dir, &names, now + 3000).unwrap();
        assert!(!fourth.renewed);
        assert_eq!(fourth.fingerprint, third.fingerprint);

        // Within 30 days of not_after -> regenerates.
        let meta = read_meta(&dir.join(META_FILE)).unwrap();
        assert!(meta.names.contains(&"10.0.0.5".to_string()));
        assert!(meta.names.contains(&"10.0.0.6".to_string()));
        let fifth = load_or_create_in(&dir, &names, meta.not_after - RENEW_WITHIN_MS + 1).unwrap();
        assert!(fifth.renewed);
        assert_ne!(fifth.fingerprint, fourth.fingerprint);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn private_key_has_mode_600_and_dir_has_mode_700() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & PERMISSION_BITS;
        let dir = temp_dir("mode");
        let names = needed_names("0.0.0.0", None);
        load_or_create_in(&dir, &names, 1_700_000_000_000).unwrap();
        assert_eq!(mode(&dir), PRIVATE_DIR_MODE);
        assert_eq!(mode(&dir.join(KEY_FILE)), PRIVATE_FILE_MODE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redirect_drops_path_and_query_and_checks_host() {
        let hosts = HostPolicy::new("0.0.0.0", 8443, true);
        let req = b"GET /login?token=secret&dir=/tmp HTTP/1.1\r\nHost: 127.0.0.1:8443\r\n\r\n";
        let resp = http_redirect_response(req, &hosts);
        assert!(resp.starts_with("HTTP/1.1 308 "), "{resp}");
        assert!(
            resp.contains("Location: https://127.0.0.1:8443/\r\n"),
            "{resp}"
        );
        assert!(!resp.contains("secret"), "{resp}");
        assert!(!resp.contains("login"), "{resp}");

        let evil = b"GET / HTTP/1.1\r\nHost: evil.example:8443\r\n\r\n";
        let evil_resp = http_redirect_response(evil, &hosts);
        assert!(evil_resp.starts_with("HTTP/1.1 403 "), "{evil_resp}");
    }
}
