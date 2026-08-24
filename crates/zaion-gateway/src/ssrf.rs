//! SSRF protection guard (M1 TM-05).
//!
//! Validates outbound URLs before fetch: resolves the host and rejects
//! loopback, private, and link-local addresses (resolver-time IP checks).
//! The resolver is injectable so tests run without real DNS.

use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;

/// Why an outbound URL was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SsfrError {
    InvalidUrl,
    ResolutionFailed,
    LoopbackAddress,
    PrivateAddress,
    LinkLocalAddress,
    UnspecifiedAddress,
}

impl std::fmt::Display for SsfrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl => write!(f, "invalid URL"),
            Self::ResolutionFailed => write!(f, "host resolution failed"),
            Self::LoopbackAddress => write!(f, "loopback address not allowed"),
            Self::PrivateAddress => write!(f, "private address not allowed"),
            Self::LinkLocalAddress => write!(f, "link-local address not allowed"),
            Self::UnspecifiedAddress => write!(f, "unspecified address not allowed"),
        }
    }
}

type ResolverFn = dyn Fn(&str) -> Vec<IpAddr> + Send + Sync + 'static;

/// SSRF guard validating outbound URLs.
#[derive(Clone)]
pub struct SsfrGuard {
    resolver: Arc<ResolverFn>,
}

impl std::fmt::Debug for SsfrGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsfrGuard").finish()
    }
}

impl Default for SsfrGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl SsfrGuard {
    /// Guard with the default (real DNS) resolver.
    pub fn new() -> Self {
        Self::with_resolver(default_resolver)
    }

    /// Guard with a custom resolver (tests, mocks).
    pub fn with_resolver<R>(resolver: R) -> Self
    where
        R: Fn(&str) -> Vec<IpAddr> + Send + Sync + 'static,
    {
        Self {
            resolver: Arc::new(resolver),
        }
    }

    /// Validate a URL; Ok means it is safe to fetch.
    pub fn validate(&self, url: &str) -> Result<(), SsfrError> {
        let host = parse_host(url)?;
        if let Ok(ip) = host.parse::<IpAddr>() {
            return check_address(ip);
        }
        let addrs = (self.resolver)(host);
        if addrs.is_empty() {
            return Err(SsfrError::ResolutionFailed);
        }
        for addr in addrs {
            check_address(addr)?;
        }
        Ok(())
    }
}

/// Extract the host from a URL (supports http/https and scheme-less).
fn parse_host(url: &str) -> Result<&str, SsfrError> {
    let rest = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        url
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let hostport = &rest[..end];
    let host = if let Some(stripped) = hostport.strip_prefix('[').and_then(|h| h.strip_suffix(']'))
    {
        stripped // bracketed IPv6 literal, no port to split
    } else {
        hostport.rsplit_once(':').map_or(hostport, |(h, _)| h)
    };
    if host.is_empty() {
        return Err(SsfrError::InvalidUrl);
    }
    Ok(host)
}

/// Reject loopback/private/link-local/unspecified addresses.
fn check_address(addr: IpAddr) -> Result<(), SsfrError> {
    match addr {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                return Err(SsfrError::LoopbackAddress);
            }
            if v4.is_private() {
                return Err(SsfrError::PrivateAddress);
            }
            if v4.is_link_local() {
                return Err(SsfrError::LinkLocalAddress);
            }
            if v4.is_unspecified() {
                return Err(SsfrError::UnspecifiedAddress);
            }
        }
        IpAddr::V6(v6) => {
            // IPv4-mapped IPv6 (e.g. ::ffff:127.0.0.1) re-checks the
            // embedded IPv4 address so it cannot bypass IPv4 guards.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return check_address(IpAddr::V4(v4));
            }
            if v6.is_loopback() {
                return Err(SsfrError::LoopbackAddress);
            }
            if v6.is_unspecified() {
                return Err(SsfrError::UnspecifiedAddress);
            }
            // fc00::/7 = unique local (private); fe80::/10 = link-local
            if (v6.segments()[0] & 0xfe00) == 0xfc00 {
                return Err(SsfrError::PrivateAddress);
            }
            if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                return Err(SsfrError::LinkLocalAddress);
            }
        }
    }
    Ok(())
}

/// Default resolver: std DNS lookup (blocking).
fn default_resolver(host: &str) -> Vec<IpAddr> {
    (host, 80)
        .to_socket_addrs()
        .map(|iter| iter.map(|sa| sa.ip()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver_for(
        addrs: &'static [&'static str],
    ) -> impl Fn(&str) -> Vec<IpAddr> + Send + Sync + 'static {
        move |_host| addrs.iter().filter_map(|a| a.parse().ok()).collect()
    }

    #[test]
    fn rejects_loopback_literal() {
        let g = SsfrGuard::new();
        assert_eq!(
            g.validate("http://127.0.0.1/admin").unwrap_err(),
            SsfrError::LoopbackAddress
        );
        assert_eq!(
            g.validate("http://[::1]/x").unwrap_err(),
            SsfrError::LoopbackAddress
        );
    }

    #[test]
    fn rejects_private_and_linklocal_literals() {
        let g = SsfrGuard::new();
        assert_eq!(
            g.validate("http://10.0.0.5/x").unwrap_err(),
            SsfrError::PrivateAddress
        );
        assert_eq!(
            g.validate("http://192.168.1.1/x").unwrap_err(),
            SsfrError::PrivateAddress
        );
        assert_eq!(
            g.validate("http://169.254.169.254/latest").unwrap_err(),
            SsfrError::LinkLocalAddress
        );
        assert_eq!(
            g.validate("http://0.0.0.0/x").unwrap_err(),
            SsfrError::UnspecifiedAddress
        );
    }

    #[test]
    fn rejects_hostname_resolving_to_private() {
        let g = SsfrGuard::with_resolver(resolver_for(&["127.0.0.1"]));
        assert_eq!(
            g.validate("http://evil.example/x").unwrap_err(),
            SsfrError::LoopbackAddress
        );
    }

    #[test]
    fn allows_public_hostname() {
        let g = SsfrGuard::with_resolver(resolver_for(&["93.184.216.34"]));
        assert!(g.validate("http://example.com/x").is_ok());
    }

    #[test]
    fn resolution_failure_rejected() {
        let g = SsfrGuard::with_resolver(|_| vec![]);
        assert_eq!(
            g.validate("http://no-such-host.invalid/x").unwrap_err(),
            SsfrError::ResolutionFailed
        );
    }

    #[test]
    fn rejects_ipv4_mapped_ipv6_loopback() {
        // ::ffff:127.0.0.1 must not bypass the loopback guard.
        let g = SsfrGuard::new();
        assert_eq!(
            g.validate("http://[::ffff:127.0.0.1]/admin").unwrap_err(),
            SsfrError::LoopbackAddress
        );
    }

    #[test]
    fn rejects_dns_rebinding_mixed_addresses() {
        // One public + one private address: the private one must win.
        let g = SsfrGuard::with_resolver(resolver_for(&["93.184.216.34", "10.0.0.5"]));
        assert_eq!(
            g.validate("http://rebind.example/x").unwrap_err(),
            SsfrError::PrivateAddress
        );
    }

    #[test]
    fn rejects_private_ipv6_unique_local() {
        let g = SsfrGuard::new();
        assert_eq!(
            g.validate("http://[fd00::1]/x").unwrap_err(),
            SsfrError::PrivateAddress
        );
    }

    #[test]
    fn invalid_url_rejected() {
        let g = SsfrGuard::new();
        assert!(g.validate("").is_err());
    }
}
