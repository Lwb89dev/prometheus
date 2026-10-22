//! HTTP calls for a configured search endpoint and for public pages.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use lda_common::{Error, Result, WebProviderKind, WebSettings};
use tokio::sync::Mutex;
use url::Url;

use crate::extract::{extract_html, Extracted};
use crate::guard::{page_url_allowed, parse_http_url, resolve_public};
use crate::search::{parse_search_json, SearchHit};

const MAX_PAGE_BYTES: usize = 1_000_000;
const MAX_PAGE_CHARS: usize = 12_000;

pub struct WebClient {
    settings: Mutex<WebSettings>,
    http: reqwest::Client,
    last: Mutex<Option<Instant>>,
}

impl WebClient {
    pub fn new(settings: WebSettings) -> Result<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .user_agent("linux-desktop-agent/0.2")
            .build()
            .map_err(|err| Error::Network(err.to_string()))?;
        Ok(Self {
            settings: Mutex::new(settings),
            http,
            last: Mutex::new(None),
        })
    }

    pub async fn set_settings(&self, settings: WebSettings) {
        *self.settings.lock().await = settings;
    }

    pub async fn auth_env_name(&self) -> Option<String> {
        self.settings.lock().await.auth_env.clone()
    }

    pub async fn search(&self, query: &str, bearer: Option<&str>) -> Result<Vec<SearchHit>> {
        let settings = self.settings.lock().await.clone();
        if !settings.enabled {
            return Err(Error::InvalidInput(
                "web search is disabled in the configuration".into(),
            ));
        }
        let query = bounded_query(query)?;
        let endpoint = parse_http_url(&settings.endpoint)?;
        self.pace(&settings).await?;
        let url = search_url(&endpoint, &query, settings.kind)?;
        let mut request = self
            .http
            .get(url)
            .timeout(Duration::from_secs(settings.timeout_secs));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|err| Error::Network(err.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Network(format!(
                "search returned HTTP {}",
                response.status()
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|err| Error::Network(err.to_string()))?;
        parse_search_json(&body, settings.max_results as usize)
    }

    pub async fn fetch_page(&self, raw_url: &str) -> Result<Extracted> {
        let settings = self.settings.lock().await.clone();
        let url = page_url_allowed(raw_url, settings.allow_private_fetch)?;
        resolve_public(&url, settings.allow_private_fetch).await?;
        self.pace(&settings).await?;
        let response = self.follow(&settings, url.clone()).await?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !(content_type.contains("text/html")
            || content_type.contains("text/plain")
            || content_type.contains("application/xhtml"))
        {
            return Err(Error::Network("the page is not HTML or plain text".into()));
        }
        let bytes = read_capped(response).await?;
        let text = String::from_utf8_lossy(&bytes);
        let mut page = extract_html(&text, MAX_PAGE_CHARS);
        if page.title.is_empty() {
            page.title = url.to_string();
        }
        Ok(page)
    }

    async fn follow(&self, settings: &WebSettings, mut url: Url) -> Result<reqwest::Response> {
        for _ in 0..3 {
            resolve_public(&url, settings.allow_private_fetch).await?;
            let response = self.fetch_once(settings, &url).await?;
            if !response.status().is_redirection() {
                return page_status(response);
            }
            url = redirect_target(&url, &response)?;
            page_url_allowed(url.as_str(), settings.allow_private_fetch)?;
        }
        Err(Error::Network("too many redirects".into()))
    }

    async fn fetch_once(&self, settings: &WebSettings, url: &Url) -> Result<reqwest::Response> {
        self.http
            .get(url.clone())
            .timeout(Duration::from_secs(settings.timeout_secs))
            .send()
            .await
            .map_err(|err| Error::Network(err.to_string()))
    }

    async fn pace(&self, settings: &WebSettings) -> Result<()> {
        let gap = Duration::from_millis(settings.min_interval_ms);
        if gap.is_zero() {
            return Ok(());
        }
        let mut last = self.last.lock().await;
        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < gap {
                tokio::time::sleep(gap - elapsed).await;
            }
        }
        *last = Some(Instant::now());
        Ok(())
    }
}

fn page_status(response: reqwest::Response) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(Error::Network(format!(
        "page returned HTTP {}",
        response.status()
    )))
}

fn redirect_target(current: &Url, response: &reqwest::Response) -> Result<Url> {
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Error::Network("redirect had no location".into()))?;
    current
        .join(location)
        .map_err(|err| Error::Network(format!("bad redirect: {err}")))
}

fn search_url(endpoint: &Url, query: &str, kind: WebProviderKind) -> Result<Url> {
    if matches!(kind, WebProviderKind::None) {
        return Err(Error::InvalidInput("web search has no provider".into()));
    }
    let mut url = endpoint.clone();
    if matches!(kind, WebProviderKind::Searxng) && !url.path().ends_with("/search") {
        let path = format!("{}/search", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("format", "json");
    Ok(url)
}

fn bounded_query(query: &str) -> Result<String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 300 {
        return Err(Error::InvalidInput(
            "search query must be 1 to 300 characters".into(),
        ));
    }
    if query.chars().any(|ch| ch.is_control()) {
        return Err(Error::InvalidInput(
            "search query contains control characters".into(),
        ));
    }
    Ok(query.to_string())
}

async fn read_capped(response: reqwest::Response) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| Error::Network(err.to_string()))?;
        let room = MAX_PAGE_BYTES.saturating_sub(body.len());
        if room == 0 {
            break;
        }
        let take = room.min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn disabled_search_does_not_connect() {
        let client = WebClient::new(WebSettings::default()).unwrap();
        let err = client.search("kernel", None).await.unwrap_err();
        assert!(err.to_string().contains("disabled"));
    }

    #[tokio::test]
    async fn fetch_rejects_link_local_without_connecting() {
        let client = WebClient::new(WebSettings::default()).unwrap();
        let err = client
            .fetch_page("http://169.254.169.254/latest/meta-data")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("private") || err.to_string().contains("not"));
    }

    #[tokio::test]
    async fn local_page_extracts_visible_text_when_private_is_opted_in() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = socket.read(&mut buf).await;
            let body = b"<html><title>Local</title><body><p>Observed page.</p><script>nope</script></body></html>";
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
        });
        let settings = WebSettings {
            allow_private_fetch: true,
            min_interval_ms: 0,
            ..WebSettings::default()
        };
        let client = WebClient::new(settings).unwrap();
        let page = client
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await
            .unwrap();
        assert!(page.text.contains("Observed page"));
        assert!(!page.text.contains("nope"));
    }
}
