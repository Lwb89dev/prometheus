//! Address checks for page fetch. Search endpoints are checked separately.

use std::net::IpAddr;

use lda_common::{parse_endpoint, Error, Result};
use url::Url;

pub fn parse_http_url(raw: &str) -> Result<Url> {
    let url = parse_endpoint(raw)?;
    if url.port().is_some_and(|port| port == 0) {
        return Err(Error::InvalidInput("port is not allowed".into()));
    }
    Ok(url)
}

pub fn ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            let private_172 = octets[0] == 172 && (16..=31).contains(&octets[1]);
            let cgnat = octets[0] == 100 && (64..=127).contains(&octets[1]);
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_unspecified()
                || private_172
                || cgnat
                || octets[0] == 0)
        }
        IpAddr::V6(ip) => {
            let mapped = ip.to_ipv4_mapped();
            if let Some(v4) = mapped {
                return ip_is_public(IpAddr::V4(v4));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || is_ula(ip)
                || is_v6_link_local(ip))
        }
    }
}

fn is_ula(ip: std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_v6_link_local(ip: std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

pub fn host_name_blocked(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host == "metadata.google.internal"
        || host == "metadata"
}

/// Page fetches use this. Loopback and link-local stay blocked unless
/// `allow_private` is set on purpose, which the default config does not do.
pub fn page_url_allowed(raw: &str, allow_private: bool) -> Result<Url> {
    let url = parse_http_url(raw)?;
    let host = url.host_str().unwrap_or("");
    if host_name_blocked(host) && !allow_private {
        return Err(Error::PermissionDenied(
            "that host is not allowed for page fetch".into(),
        ));
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return Ok(url);
    };
    if allow_private || ip_is_public(ip) {
        return Ok(url);
    }
    Err(Error::PermissionDenied(
        "private and link-local addresses are not fetched".into(),
    ))
}

pub async fn resolve_public(url: &Url, allow_private: bool) -> Result<()> {
    if allow_private {
        return Ok(());
    }
    let host = url.host_str().unwrap_or("");
    if host_name_blocked(host) {
        return Err(Error::PermissionDenied(
            "that host is not allowed for page fetch".into(),
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip_is_public(ip) {
            return Ok(());
        }
        return Err(Error::PermissionDenied(
            "private and link-local addresses are not fetched".into(),
        ));
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let mut addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| Error::Network(format!("could not resolve host: {err}")))?;
    let Some(first) = addresses.next() else {
        return Err(Error::Network("the host did not resolve".into()));
    };
    if addresses.any(|addr| !ip_is_public(addr.ip())) || !ip_is_public(first.ip()) {
        return Err(Error::PermissionDenied(
            "the host resolves to a private address".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_metadata_and_loopback() {
        assert!(page_url_allowed("http://169.254.169.254/latest", false).is_err());
        assert!(page_url_allowed("http://127.0.0.1/", false).is_err());
        assert!(page_url_allowed("http://10.1.2.3/", false).is_err());
        assert!(page_url_allowed("http://user:pw@example.com/", false).is_err());
        assert!(page_url_allowed("file:///etc/passwd", false).is_err());
        assert!(page_url_allowed("https://example.com/docs", false).is_ok());
    }

    #[test]
    fn private_opt_in_is_explicit() {
        assert!(page_url_allowed("http://127.0.0.1:9/page", true).is_ok());
    }
}
