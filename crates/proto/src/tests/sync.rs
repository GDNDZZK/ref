//! Compares a directory with the files a test expects there, or rewrites it.

use std::collections::BTreeMap;
use std::path::Path;

const UPDATE_ENV: &str = "REF_UPDATE_FIXTURES";
const UPDATE_VALUE: &str = "1";

/// Only `REF_UPDATE_FIXTURES=1` rewrites files; `0`, an empty value or anything else does not.
fn update_requested(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| v == UPDATE_VALUE)
}

/// Files by path relative to the root (`/` separated).
pub type Tree = BTreeMap<String, String>;

pub fn read_tree(dir: &Path) -> Tree {
    let mut tree = Tree::new();
    if dir.exists() {
        collect(dir, dir, &mut tree);
    }
    tree
}

fn collect(root: &Path, dir: &Path, tree: &mut Tree) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(root, &path, tree);
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .expect("inside root")
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path).expect("read file");
        tree.insert(rel, text);
    }
}

/// Makes `dir` hold exactly `expected` (update mode) or fails when it does not.
pub fn sync(dir: &Path, expected: &Tree, what: &str) {
    if update_requested(std::env::var_os(UPDATE_ENV).as_deref()) {
        write_tree(dir, expected);
        return;
    }
    let actual = read_tree(dir);
    let mut problems = Vec::new();
    for (path, text) in expected {
        match actual.get(path) {
            None => problems.push(format!("missing: {path}")),
            Some(a) if a != text => problems.push(format!("outdated: {path}")),
            Some(_) => {}
        }
    }
    for path in actual.keys().filter(|p| !expected.contains_key(*p)) {
        problems.push(format!("stale: {path}"));
    }
    assert!(
        problems.is_empty(),
        "{what} in {} are out of date; run `{UPDATE_ENV}=1 cargo test -p ref-proto`\n{}",
        dir.display(),
        problems.join("\n")
    );
}

fn write_tree(dir: &Path, expected: &Tree) {
    for path in read_tree(dir).keys().filter(|p| !expected.contains_key(*p)) {
        std::fs::remove_file(dir.join(path)).expect("remove stale file");
    }
    for (path, text) in expected {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(file, text).expect("write file");
    }
}

#[cfg(test)]
mod tests {
    use super::update_requested;
    use std::ffi::OsStr;

    #[test]
    fn only_one_asks_for_an_update() {
        assert!(update_requested(Some(OsStr::new("1"))));
        for other in ["", "0", "true", "yes"] {
            assert!(!update_requested(Some(OsStr::new(other))), "{other:?}");
        }
        assert!(!update_requested(None));
    }
}
