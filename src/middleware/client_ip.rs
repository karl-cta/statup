//! The address of the client, behind an optional trusted reverse proxy.
//! Shared by the request rate limiter and the sign-in limiter.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

/// The client address. With `trust_proxy`, the address the proxy reports
/// wins over the peer, which is then the proxy itself.
pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>, trust_proxy: bool) -> Option<IpAddr> {
    if trust_proxy && let Some(ip) = proxied_client(headers) {
        return Some(ip);
    }
    peer
}

/// `X-Real-IP` is written by the proxy itself. `X-Forwarded-For` and
/// `Forwarded` are appended to, after whatever the client sent, so only
/// their last valid entry is the address the proxy saw.
fn proxied_client(headers: &HeaderMap) -> Option<IpAddr> {
    real_ip(headers)
        .or_else(|| last_forwarded_for(headers))
        .or_else(|| last_forwarded(headers))
}

fn real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-real-ip")?
        .to_str()
        .ok()
        .and_then(parse_address)
}

fn last_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    header_entries(headers, "x-forwarded-for")
        .into_iter()
        .rev()
        .find_map(parse_address)
}

fn last_forwarded(headers: &HeaderMap) -> Option<IpAddr> {
    header_entries(headers, "forwarded")
        .into_iter()
        .rev()
        .find_map(forwarded_for)
}

/// Every comma separated entry of every line of a header, in order.
fn header_entries<'a>(headers: &'a HeaderMap, name: &str) -> Vec<&'a str> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(','))
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

    #[test]
    fn headers_are_ignored_without_a_trusted_proxy() {
        let map = headers(&[
            ("x-forwarded-for", "203.0.113.9"),
            ("x-real-ip", "203.0.113.9"),
        ]);
        assert_eq!(client_ip(&map, ip(PEER), false), ip(PEER));
    }

    #[test]
    fn a_spoofed_leftmost_forwarded_for_does_not_win() {
        let map = headers(&[("x-forwarded-for", "1.2.3.4, 198.51.100.7")]);
        assert_eq!(client_ip(&map, ip(PEER), true), ip("198.51.100.7"));
    }

    #[test]
    fn the_last_forwarded_for_line_holds_the_proxy_entry() {
        let map = headers(&[
            ("x-forwarded-for", "1.2.3.4"),
            ("x-forwarded-for", "5.6.7.8, 198.51.100.7"),
        ]);
        assert_eq!(client_ip(&map, ip(PEER), true), ip("198.51.100.7"));
    }

    #[test]
    fn an_invalid_trailing_entry_is_skipped() {
        let map = headers(&[("x-forwarded-for", "198.51.100.7, unknown")]);
        assert_eq!(client_ip(&map, ip(PEER), true), ip("198.51.100.7"));
    }

    #[test]
    fn real_ip_set_by_the_proxy_comes_first() {
        let map = headers(&[
            ("x-real-ip", "198.51.100.7"),
            ("x-forwarded-for", "1.2.3.4"),
        ]);
        assert_eq!(client_ip(&map, ip(PEER), true), ip("198.51.100.7"));
    }

    #[test]
    fn forwarded_header_uses_its_last_for_parameter() {
        let map = headers(&[(
            "forwarded",
            "for=1.2.3.4;proto=https, for=\"[2001:db8:cafe::17]:4711\";by=203.0.113.43",
        )]);
        assert_eq!(client_ip(&map, ip(PEER), true), ip("2001:db8:cafe::17"));
    }

    #[test]
    fn the_peer_is_used_when_the_proxy_sent_nothing_usable() {
        let map = headers(&[("x-forwarded-for", "garbage"), ("forwarded", "for=unknown")]);
        assert_eq!(client_ip(&map, ip(PEER), true), ip(PEER));
        assert_eq!(client_ip(&HeaderMap::new(), None, true), None);
    }

    #[test]
    fn addresses_with_ports_are_accepted() {
        assert_eq!(parse_address("198.51.100.7:4711"), ip("198.51.100.7"));
        assert_eq!(parse_address("[2001:db8::1]"), ip("2001:db8::1"));
        assert_eq!(parse_address("not an address"), None);
    }
}
