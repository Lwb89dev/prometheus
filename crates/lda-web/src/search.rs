//! Parse search JSON. Snippets stay marked as snippets.

use lda_common::{Error, Result, SourceLink};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published: Option<String>,
}

pub fn parse_search_json(body: &str, max_results: usize) -> Result<Vec<SearchHit>> {
    let value: Value = serde_json::from_str(body)
        .map_err(|err| Error::Network(format!("search response was not JSON: {err}")))?;
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Network("search response has no results array".into()))?;
    let mut hits = Vec::new();
    for item in results {
        if hits.len() >= max_results {
            break;
        }
        let Some(hit) = one_hit(item) else {
            continue;
        };
        if hits
            .iter()
            .any(|existing: &SearchHit| existing.url == hit.url)
        {
            continue;
        }
        hits.push(hit);
    }
    Ok(hits)
}

fn one_hit(item: &Value) -> Option<SearchHit> {
    let url = clean(item.get("url").and_then(Value::as_str)?, 2000);
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let title = clean(
        item.get("title").and_then(Value::as_str).unwrap_or(&url),
        180,
    );
    let snippet = item
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| item.get("snippet").and_then(Value::as_str))
        .map(|text| clean(text, 500))
        .unwrap_or_default();
    let published = item
        .get("publishedDate")
        .and_then(Value::as_str)
        .or_else(|| item.get("published").and_then(Value::as_str))
        .map(|text| clean(text, 80));
    Some(SearchHit {
        title,
        url,
        snippet,
        published,
    })
}

pub fn snippet_sources(hits: &[SearchHit]) -> Vec<SourceLink> {
    hits.iter()
        .map(|hit| SourceLink {
            title: hit.title.clone(),
            url: hit.url.clone(),
            kind: "snippet".to_string(),
        })
        .collect()
}

fn clean(value: &str, limit: usize) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if out.chars().count() >= limit {
            break;
        }
        if ch.is_control() {
            continue;
        }
        out.push(ch);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dedups_and_ignores_non_http() {
        let body = r#"{"results":[
            {"title":"One","url":"https://example.com/a","content":"snippet a","publishedDate":"2020-01-01"},
            {"title":"Dup","url":"https://example.com/a","content":"again"},
            {"title":"Bad","url":"javascript:alert(1)","content":"no"},
            {"title":"Two","url":"http://example.org/b","snippet":"snippet b"}
        ]}"#;
        let hits = parse_search_json(body, 5).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].snippet, "snippet a");
        assert_eq!(hits[1].snippet, "snippet b");
        assert_eq!(snippet_sources(&hits)[0].kind, "snippet");
    }
}
