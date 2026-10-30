//! Instance files: how running backends announce themselves.

use std::path::Path;

use ref_proto::InstanceInfo;

use crate::Endpoint;

/// Reads all instance files (live pids only, newest first). Removes files of dead pids.
pub(crate) fn instances(dir: &Path) -> Vec<InstanceInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(s) = std::fs::read_to_string(&p) else {
            continue;
        };
        match serde_json::from_str::<InstanceInfo>(&s) {
            Ok(i) if ref_sys::pid_alive(i.pid) => out.push(i),
            Ok(_) => {
                let _ = std::fs::remove_file(&p);
            }
            Err(_) => {}
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.started_at));
    out
}

pub(crate) fn endpoint_for(i: &InstanceInfo) -> Endpoint {
    let host = match i.bind.as_str() {
        "0.0.0.0" | "" | "::" | "[::]" => "127.0.0.1".to_string(),
        h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
        h => h.to_string(),
    };
    Endpoint {
        url: format!("ws://{host}:{}/", i.port),
        token: Some(i.token.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_from_instance() {
        let s = r#"{"pid":1,"port":12345,"bind":"0.0.0.0","allow_hosts":["*"],"idle_timeout":0,"started_at":5,"version":"0.1","token":"abc"}"#;
        let i: InstanceInfo = serde_json::from_str(s).unwrap();
        let e = endpoint_for(&i);
        assert_eq!(e.url, "ws://127.0.0.1:12345/");
        assert_eq!(e.token.as_deref(), Some("abc"));
        assert!(serde_json::from_str::<InstanceInfo>("{bad").is_err());
    }

    #[test]
    fn instance_dir_scan() {
        let dir = std::env::temp_dir().join(format!("ref-client-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let me = std::process::id();
        let live = format!(
            r#"{{"pid":{me},"port":1,"bind":"127.0.0.1","allow_hosts":[],"idle_timeout":0,"started_at":1,"version":"x","token":"t"}}"#
        );
        std::fs::write(dir.join("a.json"), live).unwrap();
        let dead = r#"{"pid":2147483000,"port":2,"bind":"127.0.0.1","allow_hosts":[],"idle_timeout":0,"started_at":2,"version":"x","token":"t"}"#;
        std::fs::write(dir.join("b.json"), dead).unwrap();
        let v = instances(&dir);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].port, 1);
        assert!(!dir.join("b.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
