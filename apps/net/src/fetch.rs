//! A web page as text. https only, public addresses only (checked at each redirect and after the name
//! is looked up, so a page cannot send Kestrel to this laptop's router or service), and capped.

use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

use kestrel_net_proto::https_host;
use serde_json::{Value, json};

/// The most of a page that is read.
const MAX_BYTES: u64 = 2 << 20;
const MAX_REDIRECTS: usize = 3;
const TIME_LIMIT: Duration = Duration::from_secs(20);

/// Is this address on the public internet?
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_unspecified()
                || v.is_multicast()
                // Shared address space (carrier NAT), and this network.
                || (o[0] == 100 && (64..128).contains(&o[1]))
                || o[0] == 0)
        }
        IpAddr::V6(v) => {
            if let Some(mapped) = v.to_ipv4_mapped() {
                return is_public(mapped.into());
            }
            let first = v.segments()[0];
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                // Unique local, link local.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// Why this host may not be asked, if so: a name for this machine or its network.
pub fn refused_name(host: &str) -> Option<String> {
    let host = host.to_lowercase();
    if let Ok(ip) = host.parse::<IpAddr>() {
        return (!is_public(ip)).then(|| format!("{host} is not on the public internet"));
    }
    let local = !host.contains('.')
        || [".localhost", ".local", ".internal", ".lan", ".home"]
            .iter()
            .any(|end| host.ends_with(end));
    local.then(|| format!("{host} is not on the public internet"))
}

/// The address `location` points to, from `current`: only a whole https address or a path here.
pub fn next_url(current: &str, location: &str) -> Option<String> {
    if location.starts_with("https://") {
        https_host(location)?;
        return Some(location.to_string());
    }
    if let Some(rest) = location.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if location.starts_with('/') {
        let rest = current.strip_prefix("https://")?;
        let authority = rest.split(['/', '?', '#']).next()?;
        return Some(format!("https://{authority}{location}"));
    }
    None
}

/// May this address be asked at all: its name, and every address the name stands for?
pub fn check(url: &str) -> Result<(), String> {
    let host = https_host(url).ok_or("only https addresses")?;
    if let Some(why) = refused_name(&host) {
        return Err(why);
    }
    let found = (host.as_str(), 443u16)
        .to_socket_addrs()
        .map_err(|e| format!("{host} could not be found: {e}"))?;
    match found.into_iter().find(|a| !is_public(a.ip())) {
        Some(_) => Err(format!("{host} is not on the public internet")),
        None => Ok(()),
    }
}

/// A JSON answer from an address already checked, capped like a page; its status with it. No redirects.
pub fn get_json(url: &str, params: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIME_LIMIT))
        .build()
        .into();
    let mut request = agent
        .get(url)
        .header("User-Agent", "Kestrel")
        .header("Accept", "application/json");
    for (key, value) in params {
        request = request.query(*key, *value);
    }
    let mut response = request
        .call()
        .map_err(|e| format!("the request failed: {e}"))?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("the answer could not be read: {e}"))?;
    Ok((
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    ))
}

/// The internet, for a download (src/download.rs).
pub struct Internet;

impl crate::download::Source for Internet {
    fn check(&self, url: &str) -> Result<(), String> {
        check(url)
    }

    fn get(&self, url: &str, from: u64) -> Result<crate::download::Reply, String> {
        // No limit on the whole body: a model takes as long as it takes, and the core stops this
        // program when the file stops growing.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(TIME_LIMIT))
            .timeout_recv_response(Some(TIME_LIMIT))
            .build()
            .into();
        let mut request = agent.get(url).header("User-Agent", "Kestrel");
        if from > 0 {
            request = request.header("Range", format!("bytes={from}-"));
        }
        let response = request
            .call()
            .map_err(|e| format!("the request failed: {e}"))?;
        let text = |name: &str| {
            let value = response.headers().get(name)?;
            value.to_str().ok().map(String::from)
        };
        let (location, content_range) = (text("location"), text("content-range"));
        let status = response.status().as_u16();
        Ok(crate::download::Reply {
            status,
            location,
            content_range,
            body: Box::new(response.into_body().into_reader()),
        })
    }
}

pub fn fetch(url: &str) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIME_LIMIT))
        .build()
        .into();
    let mut at = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        check(&at)?;
        let mut response = agent
            .get(&at)
            .header("User-Agent", "Kestrel")
            .call()
            .map_err(|e| format!("the request failed: {e}"))?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("a redirect without an address")?;
            at = next_url(&at, location).ok_or("a redirect away from https")?;
            continue;
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("the page could not be read: {e}"))?;
        let cut = bytes.len() as u64 > MAX_BYTES;
        bytes.truncate(MAX_BYTES as usize);
        let text = String::from_utf8_lossy(&bytes).into_owned();
        return Ok(json!({"status": status, "url": at, "text": text, "cut": cut}));
    }
    Err("too many redirects".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_are_public() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "93.184.216.34",
            "1.1.1.1",
            "172.32.0.1",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn names_for_this_machine_and_its_network_are_refused() {
        for host in [
            "localhost",
            "LOCALHOST",
            "foo.localhost",
            "printer.local",
            "nas.internal",
            "router",
            "127.0.0.1",
            "192.168.1.1",
            "::1",
        ] {
            assert!(refused_name(host).is_some(), "{host}");
        }
        for host in ["example.com", "api.open-meteo.com", "1.1.1.1"] {
            assert_eq!(refused_name(host), None, "{host}");
        }
    }

    #[test]
    fn a_redirect_stays_on_https() {
        let at = "https://example.com/a/b?c=1";
        assert_eq!(
            next_url(at, "https://other.org/x").as_deref(),
            Some("https://other.org/x")
        );
        assert_eq!(next_url(at, "/y").as_deref(), Some("https://example.com/y"));
        assert_eq!(next_url(at, "http://example.com/"), None);
        assert_eq!(next_url(at, "file:///etc/passwd"), None);
        assert_eq!(
            next_url(at, "//evil.org/x").as_deref(),
            Some("https://evil.org/x")
        );
        assert_eq!(next_url(at, "z"), None);
    }

    #[test]
    fn a_page_that_is_not_https_or_public_is_refused_before_any_request() {
        for url in [
            "http://example.com/",
            "https://localhost/",
            "https://192.168.1.1/",
            "ftp://x/",
        ] {
            assert!(fetch(url).is_err(), "{url}");
        }
    }
}
