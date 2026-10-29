//! Small OS layer shared by the backend and the TUI: process liveness, termination,
//! detaching spawned processes and killing a command's process tree.

use std::process::Command;

/// Whether a process with this pid exists.
pub fn pid_alive(pid: u32) -> bool {
    imp::pid_alive(pid)
}

/// Asks a process to exit (SIGTERM on Unix, TerminateProcess on Windows). True if delivered.
pub fn terminate(pid: u32) -> bool {
    imp::terminate(pid)
}

/// Makes `cmd` start detached from our terminal: a new session on Unix, a new process group
/// without a console window on Windows.
pub fn detach(cmd: &mut Command) {
    imp::detach(cmd)
}

/// Kills the process tree started as `pid`. On Unix `pid` must lead its own process group
/// (spawned with `process_group(0)`); `force` selects SIGKILL over SIGTERM. Windows has no
/// graceful equivalent for console programs, so the tree is always killed forcibly there.
pub fn kill_tree(pid: u32, force: bool) {
    imp::kill_tree(pid, force)
}

/// Program path for `Command::new`. On Windows a bare name like `npx` only finds `npx.exe`,
/// so names without an extension are looked up in PATH with every PATHEXT extension
/// (`npx.cmd`). Elsewhere, and when nothing matches, the name is returned unchanged.
pub fn resolve_program(program: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    if let Some(p) = find_in_path(program, std::env::var_os("PATH"), std::env::var_os("PATHEXT")) {
        return p;
    }
    std::path::PathBuf::from(program)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn find_in_path(
    program: &str,
    path: Option<std::ffi::OsString>,
    pathext: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(program);
    if p.extension().is_some() || p.components().count() != 1 {
        return None;
    }
    let exts = pathext.map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    std::env::split_paths(&path?).find_map(|dir| {
        exts.split(';').filter(|e| !e.is_empty()).map(|e| dir.join(format!("{program}{e}"))).find(|c| c.is_file())
    })
}

#[cfg(unix)]
mod imp {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    fn valid(pid: u32) -> bool {
        pid != 0 && pid <= i32::MAX as u32
    }

    pub fn pid_alive(pid: u32) -> bool {
        if !valid(pid) {
            return false;
        }
        // SAFETY: kill with signal 0 only checks for existence / permission.
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    pub fn terminate(pid: u32) -> bool {
        // SAFETY: plain kill(2).
        valid(pid) && unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } == 0
    }

    pub fn detach(cmd: &mut Command) {
        // SAFETY: setsid is async-signal-safe; detaches from our terminal session.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    pub fn kill_tree(pid: u32, force: bool) {
        if valid(pid) {
            let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
            // SAFETY: plain syscall; a negative pid targets the process group.
            unsafe {
                libc::kill(-(pid as libc::pid_t), sig);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE, TerminateProcess,
    };

    pub fn pid_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        // SAFETY: the handle is checked for null and closed before returning.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                // The process exists but belongs to someone we may not query.
                return GetLastError() == ERROR_ACCESS_DENIED;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            !ok || code == STILL_ACTIVE as u32
        }
    }

    pub fn terminate(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        // SAFETY: the handle is checked for null and closed before returning.
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                return false;
            }
            let ok = TerminateProcess(h, 1) != 0;
            CloseHandle(h);
            ok
        }
    }

    pub fn detach(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    pub fn kill_tree(pid: u32, _force: bool) {
        if pid == 0 {
            return;
        }
        // taskkill walks the parent/child relation, which covers what a shell started.
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lookup_with_extensions() {
        let dir = std::env::temp_dir().join(format!("ref-sys-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tool.cmd"), "").unwrap();
        let path = Some(dir.clone().into_os_string());
        // Lower case so the test also passes on case-sensitive file systems.
        let ext = Some(".exe;.cmd".into());
        assert_eq!(find_in_path("tool", path.clone(), ext.clone()), Some(dir.join("tool.cmd")));
        assert_eq!(find_in_path("tool.cmd", path.clone(), ext.clone()), None);
        assert_eq!(find_in_path("missing", path, ext), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn own_pid_is_alive() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }

    #[test]
    fn terminate_and_kill_tree() {
        #[cfg(unix)]
        let mut cmd = {
            use std::os::unix::process::CommandExt;
            let mut c = Command::new("sleep");
            c.arg("30").process_group(0);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("ping");
            c.args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null());
            c
        };
        let mut child = cmd.spawn().unwrap();
        assert!(pid_alive(child.id()));
        kill_tree(child.id(), true);
        child.wait().unwrap();

        let mut child = cmd.spawn().unwrap();
        assert!(terminate(child.id()));
        child.wait().unwrap();
    }
}
