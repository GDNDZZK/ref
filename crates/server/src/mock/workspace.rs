//! The sample workspace every mock session runs in.

use std::path::Path;

const PARSER_RS: &str = include_str!("assets/parser.rs.txt");
const LIB_RS: &str = include_str!("assets/lib.rs.txt");
const ROWS_PARSER_RS: &str = include_str!("assets/rows_parser.rs.txt");
const ROWS_LIB_RS: &str = include_str!("assets/rows_lib.rs.txt");
const README: &str = include_str!("assets/README.md");
const DESIGN: &str = include_str!("assets/design.md");
const NOTES: &str = "Things to remember\n\n- The parser panics on empty input.\n- Benchmarks live in benches/.\n- TODO: write the changelog.\n";
const LOGO_SIZE: u32 = 96;
const BIG_LOG_LINES: u32 = 400;

/// Writes the sample files into `dir`.
pub fn create(dir: &Path) -> std::io::Result<()> {
    let files = [
        ("README.md", README),
        ("NOTES.txt", NOTES),
        ("src/parser.rs", PARSER_RS),
        ("src/lib.rs", LIB_RS),
        ("docs/design.md", DESIGN),
        // A second small crate in `scratch/`, where edits need no approval (`stale`, `outage`).
        ("scratch/src/parser.rs", ROWS_PARSER_RS),
        ("scratch/src/lib.rs", ROWS_LIB_RS),
    ];
    for (path, text) in files {
        write(dir, path, text.as_bytes())?;
    }
    let log: String = (1..=BIG_LOG_LINES)
        .map(|i| {
            format!(
                "2025-10-09T08:{:02}:{:02}Z INFO request {i} handled in {} ms\n",
                i / 60 % 60,
                i % 60,
                3 + i % 17
            )
        })
        .collect();
    write(dir, "data/big.log", log.as_bytes())?;
    write(dir, "logo.png", &logo())?;
    std::fs::create_dir_all(dir.join("scratch"))
}

fn write(dir: &Path, rel: &str, bytes: &[u8]) -> std::io::Result<()> {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}

/// A diagonal gradient with a lighter ring.
fn logo() -> Vec<u8> {
    let center = LOGO_SIZE as f32 / 2.0;
    super::png::encode(LOGO_SIZE, LOGO_SIZE, |x, y| {
        let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
        let ring = (distance - center * 0.6).abs() < 4.0;
        if ring {
            return [250, 250, 245];
        }
        let t = (x + y) as f32 / (2 * LOGO_SIZE) as f32;
        [
            (60.0 + 140.0 * t) as u8,
            (110.0 + 60.0 * (1.0 - t)) as u8,
            200,
        ]
    })
}
