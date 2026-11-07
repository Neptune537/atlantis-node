//! # Egress Network Guard & SSRF Protection (`atlantis_protocol::egress_guard`)
//!
//! Clean-room Rust implementation of DNS-rebinding and SSRF protection.
//! Prevents sandboxed plugins, mini-apps, and remote MCP connections from probing
//! local LAN infrastructure, loopback addresses, link-local metadata endpoints,
//! or private home subnet services (`10.0.0.0/8`, `192.168.0.0/16`, `172.16.0.0/12`, `169.254.169.254`).
//!
//! Enforced per-request and on any HTTP redirect before socket binding.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Evaluates whether an IP address belongs to private, loopback, link-local,
/// CGNAT, or reserved network spaces.
pub fn is_private_or_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 0.0.0.0/8 Current network
            octets[0] == 0
                // 10.0.0.0/8 Private network
                || octets[0] == 10
                // 127.0.0.0/8 Loopback
                || octets[0] == 127
                // 100.64.0.0/10 Shared Address Space / CGNAT
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                // 169.254.0.0/16 Link-local (Cloud metadata services like AWS/GCP 169.254.169.254)
                || (octets[0] == 169 && octets[1] == 254)
                // 172.16.0.0/12 Private network
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                // 192.168.0.0/16 Private network
                || (octets[0] == 192 && octets[1] == 168)
                // 198.18.0.0/15 Benchmarking
                || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                // 224.0.0.0/4 Multicast & 240.0.0.0/4 Reserved
                || octets[0] >= 224
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            // Loopback ::1
            v6 == Ipv6Addr::LOCALHOST
                // Unspecified ::
                || v6 == Ipv6Addr::UNSPECIFIED
                // IPv4-mapped IPv6 ::ffff:a.b.c.d
                || if let Some(v4) = v6.to_ipv4_mapped() {
                    is_private_or_restricted_ip(IpAddr::V4(v4))
                } else {
                    false
                }
                // Unique Local Addresses (fc00::/7)
                || (segments[0] & 0xfe00) == 0xfc00
                // Link-local Unicast (fe80::/10)
                || (segments[0] & 0xffc0) == 0xfe80
                // Multicast (ff00::/8)
                || (segments[0] & 0xff00) == 0xff00
        }
    }
}

/// Evaluates whether an IP address is explicitly loopback (`127.0.0.0/8` or `::1`).
pub fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                true
            } else if let Some(v4) = v6.to_ipv4_mapped() {
                v4.is_loopback()
            } else {
                false
            }
        }
    }
}

/// Egress validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressSecurityError {
    PrivateIpRestricted(String),
    CloudMetadataBlocked(String),
    DnsLookupFailed(String),
    DisallowedHost(String),
}

impl std::fmt::Display for EgressSecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrivateIpRestricted(ip) => write!(f, "Egress to private/restricted IP blocked: {}", ip),
            Self::CloudMetadataBlocked(ip) => write!(f, "Egress to cloud metadata service blocked: {}", ip),
            Self::DnsLookupFailed(h) => write!(f, "DNS resolution failed for host: {}", h),
            Self::DisallowedHost(h) => write!(f, "Host is not in authorized egress whitelist: {}", h),
        }
    }
}

impl std::error::Error for EgressSecurityError {}

/// Network guard verifying outbound addresses against SSRF and private network policies.
#[derive(Debug, Clone, Default)]
pub struct EgressNetworkGuard {
    pub allow_loopback_for_local_mcp: bool,
}

impl EgressNetworkGuard {
    pub fn new(allow_loopback_for_local_mcp: bool) -> Self {
        Self {
            allow_loopback_for_local_mcp,
        }
    }

    /// Verifies whether an IP address is safe for egress traffic.
    pub fn assert_safe_ip(&self, ip: IpAddr) -> Result<(), EgressSecurityError> {
        // Special case: Local MCP server or admin daemon on loopback if explicitly permitted
        if self.allow_loopback_for_local_mcp && is_loopback_ip(ip) {
            return Ok(());
        }

        // Direct check for link-local metadata (169.254.169.254)
        if let IpAddr::V4(v4) = ip {
            if v4 == Ipv4Addr::new(169, 254, 169, 254) {
                return Err(EgressSecurityError::CloudMetadataBlocked(ip.to_string()));
            }
        }

        if is_private_or_restricted_ip(ip) {
            return Err(EgressSecurityError::PrivateIpRestricted(ip.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_private_and_metadata_ip_detection() {
        let guard = EgressNetworkGuard::new(false);

        // Public IPs (Cloudflare 1.1.1.1, Google 8.8.8.8) are safe
        assert!(guard.assert_safe_ip(IpAddr::from_str("1.1.1.1").unwrap()).is_ok());
        assert!(guard.assert_safe_ip(IpAddr::from_str("8.8.8.8").unwrap()).is_ok());

        // Private IPv4 ranges
        assert!(guard.assert_safe_ip(IpAddr::from_str("127.0.0.1").unwrap()).is_err());
        assert!(guard.assert_safe_ip(IpAddr::from_str("10.0.0.237").unwrap()).is_err());
        assert!(guard.assert_safe_ip(IpAddr::from_str("192.168.1.1").unwrap()).is_err());
        assert!(guard.assert_safe_ip(IpAddr::from_str("172.16.5.10").unwrap()).is_err());

        // Cloud metadata IP
        let metadata_err = guard.assert_safe_ip(IpAddr::from_str("169.254.169.254").unwrap());
        assert_eq!(
            metadata_err,
            Err(EgressSecurityError::CloudMetadataBlocked("169.254.169.254".into()))
        );

        // IPv6 Link-local and loopback
        assert!(guard.assert_safe_ip(IpAddr::from_str("::1").unwrap()).is_err());
        assert!(guard.assert_safe_ip(IpAddr::from_str("fe80::1").unwrap()).is_err());
    }

    #[test]
    fn test_mcp_loopback_policy() {
        let mcp_guard = EgressNetworkGuard::new(true);
        // Loopback is permitted for local MCP
        assert!(mcp_guard.assert_safe_ip(IpAddr::from_str("127.0.0.1").unwrap()).is_ok());
        // LAN is still strictly blocked
        assert!(mcp_guard.assert_safe_ip(IpAddr::from_str("192.168.1.50").unwrap()).is_err());
    }
}
