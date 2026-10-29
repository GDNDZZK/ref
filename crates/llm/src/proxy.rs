//! HTTP proxy selection from the environment.
//!
//! reqwest's built-in env proxy support only understands plain domain / IP / CIDR entries in
//! `NO_PROXY`, so common shell-style patterns (`127.*`, `*.local`, `172.2*`, `*zhihu.com`) are
//! ignored and local providers end up behind the proxy (which then answers e.g. `502` for a
//! refused connection). This module implements the selection itself:
//!
//! - loopback, `localhost`, private / link-local / unique-local IPs never use a proxy;
//! - `NO_PROXY` / `no_proxy` entries: `*` (everything), IPs, CIDRs (`10.0.0.0/8`), domains
//!   (`example.com` and `.example.com` match the domain and its subdomains), and `*` / `?`
//!   wildcards matched against the whole host (`127.*`, `*.local`, `*zhihu.com`). An optional
//!   `:port` suffix is ignored;
//! - otherwise `HTTPS_PROXY` (https URLs) / `HTTP_PROXY` (http URLs), falling back to
//!   `ALL_PROXY`; upper case wins over lower case. Only `http://` / `https://` proxies are
//!   supported (reqwest is built without SOCKS); others are ignored with a warning.

use reqwest::Url;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub struct ProxyEnv {
    pub http: Option<Url>,
    pub https: Option<Url>,
    pub all: Option<Url>,
    pub no_proxy: Vec<String>,
}

impl ProxyEnv {
    /// Read the process environment.
    pub fn from_env() -> ProxyEnv {
        Self::from_lookup(&|k| std::env::var(k).ok())
    }

    pub fn from_lookup(env: &dyn Fn(&str) -> Option<String>) -> ProxyEnv {
        let get = |names: &[&str]| names.iter().find_map(|n| env(n).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()));
        // CGI: `HTTP_PROXY` may come from a request header (httpoxy); ignore it there.
        let cgi = env("REQUEST_METHOD").is_some();
        let http = if cgi { get(&["http_proxy"]) } else { get(&["HTTP_PROXY", "http_proxy"]) };
        ProxyEnv {
            http: http.and_then(|v| parse_proxy("HTTP_PROXY", &v)),
            https: get(&["HTTPS_PROXY", "https_proxy"]).and_then(|v| parse_proxy("HTTPS_PROXY", &v)),
            all: get(&["ALL_PROXY", "all_proxy"]).and_then(|v| parse_proxy("ALL_PROXY", &v)),
            no_proxy: get(&["NO_PROXY", "no_proxy"])
                .map(|v| v.split(',').map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()).collect())
                .unwrap_or_default(),
        }
    }

    /// Proxy to use for `url`, or None for a direct connection.
    pub fn proxy_for(&self, url: &Url) -> Option<Url> {
        let host = url.host_str()?;
        if bypass(host, &self.no_proxy) {
            return None;
        }
        match url.scheme() {
            "https" | "wss" => self.https.clone().or_else(|| self.all.clone()),
            "http" | "ws" => self.http.clone().or_else(|| self.all.clone()),
            _ => None,
        }
    }
}

fn parse_proxy(var: &str, v: &str) -> Option<Url> {
    let with_scheme = if v.contains("://") { v.to_string() } else { format!("http://{v}") };
    match Url::parse(&with_scheme) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => Some(u),
        Ok(u) => {
            tracing::warn!("{var}: unsupported proxy scheme {:?}; ignored", u.scheme());
            None
        }
        Err(e) => {
            tracing::warn!("{var}: invalid proxy URL: {e}; ignored");
            None
        }
    }
}

/// Process-wide proxy settings (read once).
pub fn env() -> &'static ProxyEnv {
    static ENV: OnceLock<ProxyEnv> = OnceLock::new();
    ENV.get_or_init(ProxyEnv::from_env)
}

/// reqwest proxy implementing [`ProxyEnv::proxy_for`] for the process environment.
pub fn reqwest_proxy() -> reqwest::Proxy {
    reqwest::Proxy::custom(|url| env().proxy_for(url))
}

/// The proxy a request to `url` goes through (credentials removed), for error messages.
pub fn describe_proxy_for(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    let mut p = env().proxy_for(&u)?;
    let _ = p.set_username("");
    let _ = p.set_password(None);
    Some(p.to_string())
}

/// Hosts that are always reached directly: loopback, localhost, private networks.
pub fn is_local_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    match h.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => local_v4(v4),
        Ok(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => local_v4(v4),
            None => local_v6(v6),
        },
        Err(_) => false,
    }
}

fn local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn local_v6(ip: Ipv6Addr) -> bool {
    let seg = ip.segments()[0];
    ip.is_loopback() || ip.is_unspecified() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
}

/// Direct connection for `host`? (local hosts, or a matching `NO_PROXY` entry)
pub fn bypass(host: &str, no_proxy: &[String]) -> bool {
    if is_local_host(host) {
        return true;
    }
    let h = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    let ip = h.parse::<IpAddr>().ok();
    no_proxy.iter().any(|e| entry_matches(e, &h, ip))
}

fn entry_matches(entry: &str, host: &str, ip: Option<IpAddr>) -> bool {
    if entry == "*" {
        return true;
    }
    let entry = strip_port(entry);
    if entry.is_empty() {
        return false;
    }
    if let (Some(ip), Some((net, bits))) = (ip, entry.split_once('/')) {
        return cidr_contains(net, bits, ip);
    }
    if entry.contains('*') || entry.contains('?') {
        return glob(entry, host);
    }
    if let (Some(ip), Ok(e)) = (ip, entry.parse::<IpAddr>()) {
        return ip == e;
    }
    let domain = entry.trim_start_matches('.');
    host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'))
}

/// `host:port` -> `host`; `[v6]:port` -> `v6`; bare IPv6 is kept.
fn strip_port(e: &str) -> &str {
    if let Some(rest) = e.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match e.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') && p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => e,
    }
}

fn cidr_contains(net: &str, bits: &str, ip: IpAddr) -> bool {
    let (Ok(net), Ok(bits)) = (net.parse::<IpAddr>(), bits.parse::<u32>()) else { return false };
    match (net, ip) {
        (IpAddr::V4(n), IpAddr::V4(i)) if bits <= 32 => {
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
            u32::from(n) & mask == u32::from(i) & mask
        }
        (IpAddr::V6(n), IpAddr::V6(i)) if bits <= 128 => {
            let mask = if bits == 0 { 0 } else { u128::MAX << (128 - bits) };
            u128::from(n) & mask == u128::from(i) & mask
        }
        _ => false,
    }
}

/// Shell-style wildcard match of the whole string (`*` any run, `?` one char).
fn glob(pat: &str, s: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn penv(vars: &[(&str, &str)]) -> ProxyEnv {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        ProxyEnv::from_lookup(&move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
    }

    fn via(e: &ProxyEnv, url: &str) -> Option<String> {
        e.proxy_for(&Url::parse(url).unwrap()).map(|u| u.to_string())
    }

    const NO: &str = "*zhihu.com,*zhimg.com,100ime-iat-api.xfyun.cn,localhost,*.local,127.*,10.*,172.16.*,172.2*,192.168.*,.example.com,corp.net:8080,fd00::/8,203.0.113.0/24";

    #[test]
    fn user_environment() {
        let e = penv(&[
            ("HTTP_PROXY", "http://127.0.0.1:7890/"),
            ("HTTPS_PROXY", "http://127.0.0.1:7890/"),
            ("ALL_PROXY", "socks://127.0.0.1:7890/"),
            ("NO_PROXY", NO),
        ]);
        assert_eq!(e.all, None, "socks is unsupported and ignored");
        let p = Some("http://127.0.0.1:7890/".to_string());
        // Local / private: always direct.
        for u in [
            "http://127.0.0.1:9/v1",
            "http://localhost:8080/v1",
            "http://[::1]:9/",
            "http://10.1.2.3/",
            "http://172.20.0.1/",
            "http://192.168.1.10:11434/v1",
            "http://169.254.1.1/",
            "http://[fd12::1]/",
            "http://api.localhost/",
        ] {
            assert_eq!(via(&e, u), None, "{u}");
        }
        // NO_PROXY patterns.
        for u in [
            "https://www.zhihu.com/",
            "https://zhihu.com/",
            "https://pic1.zhimg.com/",
            "https://100ime-iat-api.xfyun.cn/",
            "http://printer.local/",
            "https://example.com/",
            "https://api.example.com/",
            "https://corp.net/",
            "https://203.0.113.7/",
        ] {
            assert_eq!(via(&e, u), None, "{u}");
        }
        // Everything else goes through the proxy.
        for u in ["https://api.openai.com/v1", "http://example.org/", "https://notexample.com/", "https://8.8.8.8/", "https://local.dev/"] {
            assert_eq!(via(&e, u), p, "{u}");
        }
    }

    #[test]
    fn scheme_selection_and_fallbacks() {
        let e = penv(&[("http_proxy", "proxy-h:3128"), ("ALL_PROXY", "http://all:1")]);
        assert_eq!(via(&e, "http://a.com/"), Some("http://proxy-h:3128/".into()));
        assert_eq!(via(&e, "https://a.com/"), Some("http://all:1/".into()));
        let e = penv(&[("HTTPS_PROXY", "http://u:p@s:2"), ("NO_PROXY", "*")]);
        assert_eq!(via(&e, "https://a.com/"), None);
        let e = penv(&[("HTTPS_PROXY", "http://u:p@s:2")]);
        assert_eq!(via(&e, "http://a.com/"), None);
        assert_eq!(via(&e, "https://a.com/"), Some("http://u:p@s:2/".into()));
        // CGI: HTTP_PROXY is ignored.
        let e = penv(&[("HTTP_PROXY", "http://evil:1"), ("REQUEST_METHOD", "GET")]);
        assert_eq!(via(&e, "http://a.com/"), None);
        assert_eq!(penv(&[]).proxy_for(&Url::parse("https://a.com").unwrap()), None);
    }

    #[test]
    fn glob_matching() {
        assert!(glob("127.*", "127.0.0.1"));
        assert!(glob("*.local", "a.b.local"));
        assert!(!glob("*.local", "local"));
        assert!(glob("172.2*", "172.25.3.4"));
        assert!(!glob("172.2*", "172.3.0.1"));
        assert!(glob("a?c", "abc"));
        assert!(glob("*", ""));
        assert!(!glob("a*b", "acbx"));
    }

    #[test]
    fn port_and_cidr_entries() {
        assert_eq!(strip_port("corp.net:8080"), "corp.net");
        assert_eq!(strip_port("[::1]:80"), "::1");
        assert_eq!(strip_port("fe80::1"), "fe80::1");
        assert!(cidr_contains("10.0.0.0", "8", "10.200.1.1".parse().unwrap()));
        assert!(!cidr_contains("10.0.0.0", "8", "11.0.0.1".parse().unwrap()));
        assert!(cidr_contains("0.0.0.0", "0", "8.8.8.8".parse().unwrap()));
    }
}
