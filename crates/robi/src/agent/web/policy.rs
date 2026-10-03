//! Refuse hosts that are not on the public internet.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const METADATA_HOSTS: &[&str] = &[
    "metadata.google.internal",
    "metadata.google.com",
    "metadata.azure.com",
    "instance-data",
    "instance-data.ec2.internal",
];

/// Why `host` and its addresses cannot be fetched.
///
/// An empty address list fails closed. Loopback, private, link-local,
/// multicast, unspecified, and carrier-grade NAT addresses are refused, as
/// are the well-known cloud metadata names and addresses.
pub fn screen_host(host: &str, addrs: &[IpAddr]) -> Result<(), String> {
    let name = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() {
        return Err("missing host".into());
    }
    if METADATA_HOSTS.contains(&name.as_str()) {
        return Err(format!("metadata host {name}"));
    }
    if addrs.is_empty() {
        return Err(format!("host {name} did not resolve"));
    }
    for addr in addrs {
        if blocked(*addr) {
            return Err(format!("host {name} resolved to {addr}"));
        }
    }
    Ok(())
}

fn blocked(addr: IpAddr) -> bool {
    if addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_multicast()
        || is_private(addr)
        || is_link_local(addr)
    {
        return true;
    }
    match addr {
        IpAddr::V4(ip) => cgnat(ip) || ip == Ipv4Addr::new(169, 254, 169, 254),
        IpAddr::V6(ip) => ip == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254),
    }
}

fn is_private(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => ip.is_private(),
        IpAddr::V6(ip) => ip.is_unique_local(),
    }
}

fn is_link_local(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_unicast_link_local() || (ip.segments()[0] & 0xff0f) == 0xff02,
    }
}

fn cgnat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..128).contains(&octets[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_address_is_allowed() {
        screen_host("docs.rs", &[IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))]).unwrap();
    }

    #[test]
    fn private_loopback_and_metadata_are_refused() {
        let cases = [
            ("localhost", IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ("printer.local", IpAddr::V4(Ipv4Addr::new(192, 168, 1, 9))),
            ("cgnat.example", IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))),
            (
                "meta.example",
                IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            ),
        ];
        for (host, addr) in cases {
            assert!(screen_host(host, &[addr]).is_err(), "{host}");
        }
        assert!(screen_host("metadata.google.internal", &[]).is_err());
        assert!(screen_host("docs.rs", &[]).is_err());
    }
}
