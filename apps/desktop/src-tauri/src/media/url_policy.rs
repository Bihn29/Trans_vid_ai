use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use thiserror::Error;
use tokio::net::lookup_host;
use url::Url;

use crate::domain::MediaSite;

const MAX_URL_BYTES: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRemoteUrl {
    raw: String,
    host: String,
    site: MediaSite,
}

impl ValidatedRemoteUrl {
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn site(&self) -> MediaSite {
        self.site
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEndpoint {
    pub url: ValidatedRemoteUrl,
    pub addresses: Vec<SocketAddr>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UrlPolicyError {
    #[error("remote URL is invalid")]
    InvalidUrl,
    #[error("remote URL scheme is not allowed")]
    Scheme,
    #[error("remote URL credentials or port are not allowed")]
    Authority,
    #[error("remote host is not supported")]
    UnsupportedHost,
    #[error("remote redirect changed site family")]
    CrossSiteRedirect,
    #[error("remote redirect limit exceeded")]
    RedirectLimit,
    #[error("remote host did not resolve")]
    Resolution,
    #[error("remote address is not public")]
    NonPublicAddress,
}

#[derive(Debug, Clone, Copy)]
pub struct NetworkPolicy {
    max_redirects: usize,
}

impl NetworkPolicy {
    pub fn new(max_redirects: usize) -> Result<Self, UrlPolicyError> {
        if max_redirects == 0 || max_redirects > 10 {
            return Err(UrlPolicyError::RedirectLimit);
        }
        Ok(Self { max_redirects })
    }

    pub fn validate_url(&self, raw: &str) -> Result<ValidatedRemoteUrl, UrlPolicyError> {
        if raw.is_empty() || raw.len() > MAX_URL_BYTES {
            return Err(UrlPolicyError::InvalidUrl);
        }
        let parsed = Url::parse(raw).map_err(|_| UrlPolicyError::InvalidUrl)?;
        if parsed.scheme() != "https" {
            return Err(UrlPolicyError::Scheme);
        }
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port_or_known_default() != Some(443)
            || parsed.fragment().is_some()
        {
            return Err(UrlPolicyError::Authority);
        }
        let host = parsed
            .host_str()
            .ok_or(UrlPolicyError::UnsupportedHost)?
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let site = site_for_host(&host).ok_or(UrlPolicyError::UnsupportedHost)?;
        Ok(ValidatedRemoteUrl {
            raw: parsed.to_string(),
            host,
            site,
        })
    }

    pub fn validate_redirect(
        &self,
        initial: &ValidatedRemoteUrl,
        redirect_index: usize,
        raw: &str,
    ) -> Result<ValidatedRemoteUrl, UrlPolicyError> {
        if redirect_index == 0 || redirect_index > self.max_redirects {
            return Err(UrlPolicyError::RedirectLimit);
        }
        let redirect = self.validate_url(raw)?;
        if redirect.site != initial.site {
            return Err(UrlPolicyError::CrossSiteRedirect);
        }
        Ok(redirect)
    }

    pub fn validate_addresses(
        &self,
        url: ValidatedRemoteUrl,
        addresses: impl IntoIterator<Item = IpAddr>,
    ) -> Result<ResolvedEndpoint, UrlPolicyError> {
        let addresses = addresses
            .into_iter()
            .map(|address| SocketAddr::new(address, 443))
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(UrlPolicyError::Resolution);
        }
        if addresses
            .iter()
            .any(|address| !is_public_address(address.ip()))
        {
            return Err(UrlPolicyError::NonPublicAddress);
        }
        Ok(ResolvedEndpoint { url, addresses })
    }

    pub async fn resolve_and_validate(
        &self,
        url: ValidatedRemoteUrl,
    ) -> Result<ResolvedEndpoint, UrlPolicyError> {
        let addresses = lookup_host((url.host(), 443))
            .await
            .map_err(|_| UrlPolicyError::Resolution)?
            .map(|address| address.ip())
            .collect::<Vec<_>>();
        self.validate_addresses(url, addresses)
    }
}

fn site_for_host(host: &str) -> Option<MediaSite> {
    match host {
        "douyin.com" | "www.douyin.com" | "v.douyin.com" => Some(MediaSite::Douyin),
        "bilibili.com" | "www.bilibili.com" | "b23.tv" => Some(MediaSite::Bilibili),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "youtu.be" => {
            Some(MediaSite::YouTube)
        }
        "tiktok.com" | "www.tiktok.com" | "vm.tiktok.com" | "vt.tiktok.com" => {
            Some(MediaSite::TikTok)
        }
        _ => None,
    }
}

pub fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !matches!(
        (a, b, c),
        (0, _, _)
            | (10, _, _)
            | (100, 64..=127, _)
            | (127, _, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 0, _)
            | (192, 168, _)
            | (198, 18..=19, _)
            | (224..=255, _, _)
    ) && !(a == 192 && b == 0 && c == 2)
        && !(a == 192 && b == 88 && c == 99)
        && !(a == 198 && b == 51 && c == 100)
        && !(a == 203 && b == 0 && c == 113)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    let segments = address.segments();
    (segments[0] & 0xe000) == 0x2000
        && !(segments[0] == 0x2001 && segments[1] < 0x0200)
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        && segments[0] != 0x2002
        && segments[0] != 0x3ffe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_known_https_sites_without_credentials() {
        let policy = NetworkPolicy::new(5).expect("policy");
        for (url, site) in [
            ("https://www.douyin.com/video/1", MediaSite::Douyin),
            ("https://b23.tv/abc", MediaSite::Bilibili),
            ("https://youtu.be/abc", MediaSite::YouTube),
            ("https://vm.tiktok.com/abc", MediaSite::TikTok),
        ] {
            assert_eq!(policy.validate_url(url).expect("valid site").site(), site);
        }
        for url in [
            "http://youtube.com/watch?v=x",
            "https://user:pass@youtube.com/watch?v=x",
            "https://youtube.com:444/watch?v=x",
            "https://youtube.com.evil.test/watch?v=x",
            "https://127.0.0.1/video",
            "https://youtube.com/watch?v=x#fragment",
        ] {
            assert!(policy.validate_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn blocks_private_reserved_and_documentation_addresses() {
        for address in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "172.20.0.1",
            "192.168.1.1",
            "198.51.100.9",
            "203.0.113.4",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2001:10::1",
            "3ffe::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                !is_public_address(address.parse().expect("IP fixture")),
                "{address}"
            );
        }
        for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(
                is_public_address(address.parse().expect("public IP")),
                "{address}"
            );
        }
    }

    #[test]
    fn validates_every_redirect_against_site_and_limit() {
        let policy = NetworkPolicy::new(2).expect("policy");
        let initial = policy
            .validate_url("https://youtu.be/source")
            .expect("initial URL");
        assert!(policy
            .validate_redirect(&initial, 1, "https://www.youtube.com/watch?v=x")
            .is_ok());
        assert_eq!(
            policy.validate_redirect(&initial, 1, "https://www.tiktok.com/video/x"),
            Err(UrlPolicyError::CrossSiteRedirect)
        );
        assert_eq!(
            policy.validate_redirect(&initial, 3, "https://www.youtube.com/watch?v=x"),
            Err(UrlPolicyError::RedirectLimit)
        );
    }
}
