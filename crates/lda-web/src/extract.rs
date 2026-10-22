//! Turn HTML into bounded text. Scripts are dropped, never executed.

const SKIP_TAGS: &[&str] = &["script", "style", "noscript", "template", "svg", "iframe"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub title: String,
    pub published: Option<String>,
    pub text: String,
}

pub fn extract_html(input: &str, limit: usize) -> Extracted {
    let published =
        meta_content(input, "article:published_time").or_else(|| meta_content(input, "date"));
    let title = element_text(input, "title").unwrap_or_default();
    let without_blocks = remove_skipped(input);
    let stripped = strip_tags(&without_blocks);
    let text = collapse(&decode_entities(&stripped), limit);
    Extracted {
        title: collapse(&decode_entities(&title), 180),
        published: published.map(|value| collapse(&decode_entities(&value), 80)),
        text,
    }
}

fn remove_skipped(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::new();
    let mut index = 0;
    while index < input.len() {
        let Some(open) = find_skip_open(&lower, index) else {
            out.push_str(&input[index..]);
            break;
        };
        out.push_str(&input[index..open.at]);
        let close = find_close(&lower, open.name, open.after);
        index = close;
    }
    out
}

struct Open {
    at: usize,
    after: usize,
    name: &'static str,
}

fn find_skip_open(lower: &str, from: usize) -> Option<Open> {
    let mut best: Option<Open> = None;
    for name in SKIP_TAGS {
        let pattern = format!("<{name}");
        let Some(at) = lower[from..].find(&pattern) else {
            continue;
        };
        let at = from + at;
        let after_name = at + pattern.len();
        let boundary = lower.as_bytes().get(after_name).copied();
        if boundary.is_some_and(|byte| byte.is_ascii_alphanumeric()) {
            continue;
        }
        let Some(rel) = lower[after_name..].find('>') else {
            continue;
        };
        let open = Open {
            at,
            after: after_name + rel + 1,
            name,
        };
        if best.as_ref().is_none_or(|current| open.at < current.at) {
            best = Some(open);
        }
    }
    best
}

fn find_close(lower: &str, name: &str, from: usize) -> usize {
    let pattern = format!("</{name}");
    let Some(rel) = lower[from..].find(&pattern) else {
        return lower.len();
    };
    let at = from + rel;
    lower[at..]
        .find('>')
        .map(|end| at + end + 1)
        .unwrap_or(lower.len())
}

fn element_text(input: &str, name: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let open = format!("<{name}");
    let start = lower.find(&open)?;
    let after = lower[start..].find('>')? + start + 1;
    let close = format!("</{name}");
    let end = lower[after..].find(&close)? + after;
    Some(input[after..end].to_string())
}

fn meta_content(input: &str, token: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let at = lower.find(&token.to_ascii_lowercase())?;
    let window = &input[at..input.len().min(at + 300)];
    let lower_window = window.to_ascii_lowercase();
    let content_at = lower_window.find("content=")?;
    let rest = &window[content_at + "content=".len()..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

fn strip_tags(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '<' {
            out.push(ch);
            continue;
        }
        for next in chars.by_ref() {
            if next == '>' {
                break;
            }
        }
        out.push(' ');
    }
    out
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn collapse(input: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    for ch in input.chars() {
        if out.chars().count() >= limit {
            break;
        }
        if ch.is_whitespace() || ch.is_control() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_text_is_not_kept() {
        let html = r#"<html><head><title>Docs &amp; notes</title>
            <meta property="article:published_time" content="2024-01-02">
            </head><body>
            <script>ignored tool {"name":"get_kernel_info"}</script>
            <p>Visible fact.</p>
            </body></html>"#;
        let page = extract_html(html, 200);
        assert_eq!(page.title, "Docs & notes");
        assert_eq!(page.published.as_deref(), Some("2024-01-02"));
        assert!(page.text.contains("Visible fact"));
        assert!(!page.text.contains("get_kernel_info"));
    }
}
