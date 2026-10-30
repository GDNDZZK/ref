//! Embeds the web frontend (`web/dist`), pre-compressed.
//!
//! Writes `$OUT_DIR/assets.rs`: a table of `Asset { path, mime, raw, br, gz }` that
//! `src/assets.rs` includes. The build stops when `web/dist/index.html` is missing.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const DIST_FROM_MANIFEST: &str = "../../web/dist";
const INDEX: &str = "index.html";
/// Release builds compress as hard as brotli goes. Build scripts are not optimized, and at
/// quality 11 that takes ~16s for the whole frontend on every change, so other profiles use a
/// middle setting (~1s, ~10% bigger).
const BROTLI_QUALITY_RELEASE: u32 = 11;
const BROTLI_QUALITY_OTHER: u32 = 5;
const BROTLI_WINDOW_BITS: u32 = 22;
const BROTLI_BUFFER: usize = 4096;
const GZIP_LEVEL_RELEASE: u32 = 9;
const GZIP_LEVEL_OTHER: u32 = 6;

/// Extension → MIME type of everything a Vite build emits.
const MIME_TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json"),
    ("webmanifest", "application/manifest+json"),
    ("txt", "text/plain; charset=utf-8"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("ico", "image/x-icon"),
    ("webp", "image/webp"),
    ("jpg", "image/jpeg"),
    ("woff2", "font/woff2"),
    ("woff", "font/woff"),
    ("wasm", "application/wasm"),
];
const FALLBACK_MIME: &str = "application/octet-stream";
/// Formats that are not compressed already.
const COMPRESSIBLE: &[&str] = &[
    "html",
    "js",
    "mjs",
    "css",
    "json",
    "webmanifest",
    "txt",
    "svg",
    "ico",
    "wasm",
];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let dist = manifest.join(DIST_FROM_MANIFEST);
    println!("cargo::rerun-if-changed={}", dist.display());
    if !dist.join(INDEX).is_file() {
        eprintln!(
            "error: ref-web embeds the web frontend, but {} does not exist.\n\
             Build the frontend first:\n\n    cd web && pnpm install && pnpm build\n",
            dist.join(INDEX).display()
        );
        std::process::exit(1);
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it"));
    let mut files = Vec::new();
    collect_files(&dist, &mut files);
    files.sort();

    let mut table = String::from("&[\n");
    for (n, file) in files.iter().enumerate() {
        let url_path = url_path(&dist, file);
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let raw = std::fs::read(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let (br, gz) = if COMPRESSIBLE.contains(&ext) {
            (
                smaller(brotli(&raw), raw.len()),
                smaller(gzip(&raw), raw.len()),
            )
        } else {
            (None, None)
        };
        let embed = |bytes: Option<Vec<u8>>, suffix: &str| match bytes {
            None => "None".to_string(),
            Some(b) => {
                let path = out.join(format!("asset-{n}.{suffix}"));
                std::fs::write(&path, b).expect("write compressed asset");
                format!("Some(include_bytes!({:?}))", path.display().to_string())
            }
        };
        writeln!(
            table,
            "    Asset {{ path: {url_path:?}, mime: {:?}, raw: include_bytes!({:?}), br: {}, gz: {} }},",
            mime(ext),
            file.display().to_string(),
            embed(br, "br"),
            embed(gz, "gz"),
        )
        .expect("write to string");
    }
    table.push(']');
    std::fs::write(out.join("assets.rs"), table).expect("write assets.rs");
}

/// Files and directories that are not served: hidden ones (`.DS_Store`, Vite's `.vite`
/// manifest directory) and source maps (large, only useful when debugging the frontend).
fn is_skipped(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    name.starts_with('.') || name.ends_with(".map")
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("read dist entry").path();
        if is_skipped(&path) {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// `web/dist/assets/app.js` → `/assets/app.js`.
fn url_path(dist: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(dist).expect("file is under dist");
    let parts: Vec<_> = rel.iter().map(|p| p.to_string_lossy()).collect();
    format!("/{}", parts.join("/"))
}

fn mime(ext: &str) -> &'static str {
    MIME_TYPES
        .iter()
        .find(|(e, _)| *e == ext)
        .map_or(FALLBACK_MIME, |(_, m)| m)
}

fn smaller(compressed: Vec<u8>, original: usize) -> Option<Vec<u8>> {
    (compressed.len() < original).then_some(compressed)
}

fn is_release_build() -> bool {
    std::env::var("PROFILE").is_ok_and(|p| p == "release")
}

fn brotli(raw: &[u8]) -> Vec<u8> {
    let quality = if is_release_build() {
        BROTLI_QUALITY_RELEASE
    } else {
        BROTLI_QUALITY_OTHER
    };
    let mut out = Vec::new();
    {
        let mut w =
            brotli::CompressorWriter::new(&mut out, BROTLI_BUFFER, quality, BROTLI_WINDOW_BITS);
        w.write_all(raw).expect("brotli");
    }
    out
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    let level = if is_release_build() {
        GZIP_LEVEL_RELEASE
    } else {
        GZIP_LEVEL_OTHER
    };
    let mut w = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
    w.write_all(raw).expect("gzip");
    w.finish().expect("gzip")
}
