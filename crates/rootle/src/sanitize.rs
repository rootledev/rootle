//! Sanitization boundary: remote bytes are hostile terminal input.
//! Everything the UI draws from network/disk passes through here (PLAN.md §9).

/// Heuristic binary detection: NUL byte or >10% C0/C1 control chars
/// (excluding \n, \t, \r) in the first 8 KiB.
pub fn is_binary(bytes: &[u8]) -> bool {
    const WINDOW: usize = 8 * 1024;
    let sample = &bytes[..bytes.len().min(WINDOW)];
    if sample.is_empty() {
        return false;
    }
    if sample.contains(&0) {
        return true;
    }
    let control = sample
        .iter()
        .filter(|&&b| (b < 0x20 && b != b'\n' && b != b'\t' && b != b'\r') || b == 0x7f)
        .count();
    control * 10 > sample.len()
}

/// Lossy UTF-8 decode + strip C0/C1 controls except \n and \t.
/// Critically strips ESC (\x1b): a single escape byte in file content
/// would inject terminal sequences and corrupt the whole screen.
pub fn sanitize(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars()
        .filter(|&c| c == '\n' || c == '\t' || !is_control(c))
        .collect()
}

/// Single-line variant for names (file/dir entries): drops \n and \t too.
pub fn sanitize_inline(name: &str) -> String {
    name.chars().filter(|&c| !is_control(c)).collect()
}

fn is_control(c: char) -> bool {
    (c as u32) < 0x20 || (0x7f..=0x9f).contains(&(c as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escape_and_control_bytes() {
        let raw = b"hello \x1b[2Jevil\x07 world\nkeep\ttabs";
        assert_eq!(sanitize(raw), "hello [2Jevil world\nkeep\ttabs");
    }

    #[test]
    fn invalid_utf8_becomes_replacement_char() {
        let raw = b"ok \xff\xfe bytes";
        let out = sanitize(raw);
        assert!(out.contains('\u{FFFD}'));
        assert!(out.starts_with("ok "));
    }

    #[test]
    fn detects_binary_by_nul() {
        assert!(is_binary(b"\x7fELF\x00\x01\x02"));
        assert!(!is_binary(b"fn main() {}\n"));
    }

    #[test]
    fn detects_binary_by_control_ratio() {
        let mut raw = vec![0x01u8; 256];
        raw.extend_from_slice(b"some text");
        assert!(is_binary(&raw));
    }

    #[test]
    fn inline_drops_newlines_and_tabs() {
        assert_eq!(sanitize_inline("a\nb\tc\x1bd"), "abcd");
    }
}
