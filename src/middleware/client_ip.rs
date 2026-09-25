//! The address of the client, behind an optional trusted reverse proxy.
//! Shared by the request rate limiter and the sign-in limiter.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use axum::http::HeaderMap;
use axum::http::header::{FORWARDED, HeaderName};

/// The list header every common reverse proxy appends the client to.
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// Where the client address is read from.
#[derive(Clone, Debug)]
pub enum ClientIpSource {
    /// The connection itself: the app faces its clients.
    Peer,
    /// The one header the reverse proxy in front writes. Any other header
    /// may come from the client and is never read.
    Header(HeaderName),
}

/// The client address: the last entry of the configured header when it
/// holds a valid one, the peer otherwise.
pub fn client_ip(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    source: &ClientIpSource,
) -> Option<IpAddr> {
    if let ClientIpSource::Header(name) = source
        && let Some(ip) = proxied_client(headers, name)
    {
        return Some(ip);
    }
    peer
}

/// What a rate limit counts against. An IPv6 client usually holds a whole
/// /64 and could take a new address for every attempt, so the /64 counts as
/// one; an IPv4 address written as IPv6 counts as itself.
pub fn limit_key(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = ip else {
        return ip;
    };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return IpAddr::V4(v4);
    }
    let [a, b, c, d, ..] = v6.segments();
    IpAddr::V6(Ipv6Addr::new(a, b, c, d, 0, 0, 0, 0))
}

/// A proxy appends to a list header after whatever the client sent, and
/// replaces a single value header, so only the last entry is its own. An
/// unreadable last entry is not skipped: the one before it is the client's.
fn proxied_client(headers: &HeaderMap, name: &HeaderName) -> Option<IpAddr> {
    let last = header_entries(headers, name).pop()?;
    if *name == FORWARDED {
        forwarded_for(last)
    } else {
        parse_address(last)
    }
}

/// Every comma separated entry of every line of a header, in order. A line
/// that is not text counts as one unreadable entry rather than vanishing,
/// which would bring an earlier line forward.
fn header_entries<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Vec<&'a str> {
    headers
        .get_all(name)
        .iter()
        .flat_map(|value| value.to_str().unwrap_or_default().split(','))
        .collect()
}

/// The `for=` parameter of one `Forwarded` element (RFC 7239).
fn forwarded_for(element: &str) -> Option<IpAddr> {
    element.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("for") {
            return None;
        }
        parse_address(value.trim().trim_matches('"'))
    })
}

/// An address with or without a port, IPv6 bracketed or not.
fn parse_address(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    raw.parse::<IpAddr>()
        .ok()
        .or_else(|| raw.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
        .or_else(|| {
            raw.strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|inner| inner.parse().ok())
        })
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    const PEER: &str = "10.0.0.1";

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_static(value));
        }
        map
    }

    fn ip(raw: &str) -> Option<IpAddr> {
        raw.parse().ok()
    }

    fn from(name: &'static str) -> ClientIpSource {
        ClientIpSource::Header(HeaderName::from_static(name))
    }

    fn x_forwarded_for() -> ClientIpSource {
        ClientIpSource::Header(X_FORWARDED_FOR)
    }

    #[test]
    fn headers_are_ignored_without_a_trusted_proxy() {
        let map = headers(&[
            ("x-forwarded-for", "203.0.113.9"),
            ("x-real-ip", "203.0.113.9"),
        ]);
        assert_eq!(client_ip(&map, ip(PEER), &ClientIpSource::Peer), ip(PEER));
    }

    #[test]
    fn a_spoofed_leftmost_forwarded_for_does_not_win() {
        let map = headers(&[("x-forwarded-for", "1.2.3.4, 198.51.100.7")]);
        assert_eq!(
            client_ip(&map, ip(PEER), &x_forwarded_for()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn the_last_forwarded_for_line_holds_the_proxy_entry() {
        let map = headers(&[
            ("x-forwarded-for", "1.2.3.4"),
            ("x-forwarded-for", "5.6.7.8, 198.51.100.7"),
        ]);
        assert_eq!(
            client_ip(&map, ip(PEER), &x_forwarded_for()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn an_unreadable_last_entry_falls_back_to_the_peer() {
        let map = headers(&[("x-forwarded-for", "1.2.3.4, unknown")]);
        assert_eq!(client_ip(&map, ip(PEER), &x_forwarded_for()), ip(PEER));
    }

    #[test]
    fn a_header_the_proxy_does_not_write_is_never_read() {
        let map = headers(&[
            ("x-real-ip", "1.2.3.4"),
            ("x-forwarded-for", "198.51.100.7"),
        ]);
        assert_eq!(
            client_ip(&map, ip(PEER), &x_forwarded_for()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn a_named_single_value_header_is_read() {
        let map = headers(&[
            ("x-real-ip", "198.51.100.7"),
            ("x-forwarded-for", "1.2.3.4"),
        ]);
        assert_eq!(
            client_ip(&map, ip(PEER), &from("x-real-ip")),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn forwarded_header_uses_its_last_for_parameter() {
        let map = headers(&[(
            "forwarded",
            "for=1.2.3.4;proto=https, for=\"[2001:db8:cafe::17]:4711\";by=203.0.113.43",
        )]);
        assert_eq!(
            client_ip(&map, ip(PEER), &from("forwarded")),
            ip("2001:db8:cafe::17")
        );
    }

    #[test]
    fn the_peer_is_used_when_the_proxy_sent_nothing_usable() {
        let map = headers(&[("x-forwarded-for", "garbage"), ("forwarded", "for=unknown")]);
        assert_eq!(client_ip(&map, ip(PEER), &x_forwarded_for()), ip(PEER));
        assert_eq!(client_ip(&map, ip(PEER), &from("forwarded")), ip(PEER));
        assert_eq!(client_ip(&HeaderMap::new(), None, &x_forwarded_for()), None);
    }

    #[test]
    fn an_ipv6_client_counts_as_its_64_block() {
        let key = |raw: &str| ip(raw).map(limit_key);
        assert_eq!(key("2001:db8:1:2:aaaa::1"), ip("2001:db8:1:2::"));
        assert_eq!(key("2001:db8:1:2:bbbb::9"), key("2001:db8:1:2:aaaa::1"));
        assert_ne!(key("2001:db8:1:3::1"), key("2001:db8:1:2::1"));
        assert_eq!(key("::ffff:192.0.2.1"), ip("192.0.2.1"));
        assert_eq!(key("192.0.2.1"), ip("192.0.2.1"));
    }

    #[test]
    fn addresses_with_ports_are_accepted() {
        assert_eq!(parse_address("198.51.100.7:4711"), ip("198.51.100.7"));
        assert_eq!(parse_address("[2001:db8::1]"), ip("2001:db8::1"));
        assert_eq!(parse_address("not an address"), None);
    }
}
