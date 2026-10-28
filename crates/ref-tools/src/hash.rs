//! File content tag (hashline snapshot hash).

use xxhash_rust::xxh32::xxh32;

/// 4 uppercase hex chars: low 16 bits of xxh32 (seed 0) over the text with trailing
/// ' ', '\t', '\r' stripped from every line. Input is expected to be LF-normalized and BOM-stripped.
pub fn file_hash(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    for segment in text.split_inclusive('\n') {
        let (line, nl) = segment.strip_suffix('\n').map_or((segment, ""), |l| (l, "\n"));
        normalized.push_str(line.trim_end_matches([' ', '\t', '\r']));
        normalized.push_str(nl);
    }
    format!("{:04X}", xxh32(normalized.as_bytes(), 0) & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(file_hash("a \n b\t\r\nc"), "80BA");
        assert_eq!(file_hash("hello\n"), "5BF9");
        assert_eq!(file_hash(""), "5D05");
    }
}
