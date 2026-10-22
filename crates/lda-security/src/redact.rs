const MARKERS: &[&str] = &[
    "sk-ant-", "sk-", "xai-", "Bearer ", "bearer ", "api_key=", "API_KEY=", "apikey=",
];

/// Replace secret-shaped tokens. The marker itself is kept so logs stay readable.
pub fn redact_secrets(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(marker_len) = marker_len_at(input, i) {
            let token_end = token_end(bytes, i + marker_len);
            let token_len = token_end - (i + marker_len);
            if token_len >= 8 {
                out.push_str(&input[i..i + marker_len]);
                out.push_str("[redacted]");
                i = token_end;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn preview(text: &str, max_chars: usize) -> String {
    let redacted = redact_secrets(text);
    let count = redacted.chars().count();
    if count <= max_chars {
        return redacted;
    }
    let mut out: String = redacted.chars().take(max_chars).collect();
    out.push('…');
    out
}

fn marker_len_at(input: &str, index: usize) -> Option<usize> {
    let rest = &input[index..];
    MARKERS
        .iter()
        .find(|marker| rest.starts_with(**marker))
        .map(|marker| marker.len())
}

fn token_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() {
        let b = bytes[end];
        let ok = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b'+' | b'=');
        if !ok {
            break;
        }
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer_and_vendor_keys() {
        let input = "Authorization: Bearer abcdefghijklmnop and sk-abcdefghijklmnopqrstuvwxyz";
        let out = redact_secrets(input);
        assert!(!out.contains("abcdefghijklmnop"));
        assert!(!out.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(out.contains("Bearer [redacted]"));
        assert!(out.contains("sk-[redacted]"));
    }

    #[test]
    fn leaves_ordinary_text() {
        let input = "the kernel is 7.2.6 and the desk-top is COSMIC";
        assert_eq!(redact_secrets(input), input);
    }
}
