//! Generated artifacts of the protocol: JSON fixtures and TypeScript types.
//!
//! Both are written by tests. `REF_UPDATE_FIXTURES=1 cargo test -p ref-proto` rewrites them;
//! a plain test run fails when they are out of date.

mod bindings;
mod compat;
mod fixtures;
mod samples;
mod sync;

#[test]
fn home_from_requires_home_or_os_home() {
    assert_eq!(
        crate::home_from(Some("/tmp/h".into()), || None).unwrap(),
        std::path::PathBuf::from("/tmp/h")
    );
    assert_eq!(
        crate::home_from(Some("".into()), || Some("/etc/h".into())).unwrap(),
        std::path::PathBuf::from("/etc/h")
    );
    let err = crate::home_from(None, || None).unwrap_err();
    assert_eq!(err, "HOME is not set");
    let err_empty = crate::home_from(Some("".into()), || None).unwrap_err();
    assert_eq!(err_empty, "HOME is not set");
}
