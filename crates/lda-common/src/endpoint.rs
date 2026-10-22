use url::Url;

use crate::{Error, Result};

pub fn parse_endpoint(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim()).map_err(|e| Error::InvalidInput(format!("endpoint: {e}")))?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(Error::InvalidInput(format!(
                "endpoint scheme '{other}' is not allowed"
            )))
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidInput(
            "put API keys in the secret service or an environment variable, not in the endpoint URL"
                .into(),
        ));
    }
    if url.host_str().is_none() {
        return Err(Error::InvalidInput("endpoint has no host".into()));
    }
    Ok(url)
}

pub fn endpoint_is_local(raw: &str) -> bool {
    let Ok(url) = parse_endpoint(raw) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Join `base` with a relative API path without duplicating a suffix the user already set.
pub fn join_endpoint(base: &str, suffix: &str) -> Result<String> {
    let mut url = parse_endpoint(base)?;
    let path = url.path().trim_end_matches('/').to_string();
    let suffix = suffix.trim_start_matches('/');
    if path.ends_with(&format!("/{suffix}")) || path == suffix {
        return Ok(url.to_string());
    }
    let joined = if path.is_empty() || path == "/" {
        format!("/{suffix}")
    } else {
        format!("{path}/{suffix}")
    };
    url.set_path(&joined);
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts() {
        assert!(endpoint_is_local("http://127.0.0.1:11434"));
        assert!(endpoint_is_local("http://localhost/v1"));
        assert!(endpoint_is_local("http://[::1]:8080"));
        assert!(!endpoint_is_local("https://api.openai.com/v1"));
        assert!(!endpoint_is_local("http://169.254.169.254/"));
    }

    #[test]
    fn join_does_not_duplicate() {
        let url = join_endpoint("http://127.0.0.1:11434", "api/chat").unwrap();
        assert_eq!(url, "http://127.0.0.1:11434/api/chat");
        let again = join_endpoint(
            "https://example.com/v1/chat/completions",
            "chat/completions",
        )
        .unwrap();
        assert!(again.contains("/chat/completions"));
        assert_eq!(again.matches("chat/completions").count(), 1);
    }
}
