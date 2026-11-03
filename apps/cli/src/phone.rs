//! The page on the person's phone through Tailscale (v4.1, F; blueprint: free personal plan, encrypted).
//! Off at first. When it is on, the page also listens on this device's Tailscale address and on no other
//! network: not the wifi. The phone can read and ask; what changes the internet, mail or a model stays on
//! this device (`web::from_this_device`).

use std::net::Ipv4Addr;

/// A Tailscale address, from what `tailscale ip -4` prints.
pub fn tailscale_ip(output: &str) -> Option<Ipv4Addr> {
    let ip: Ipv4Addr = output.lines().next()?.trim().parse().ok()?;
    let o = ip.octets();
    (o[0] == 100 && (64..128).contains(&o[1])).then_some(ip)
}

/// The address to share the page on, when the person turned the phone page on; `ask` runs Tailscale.
pub fn address(on: bool, ask: &dyn Fn() -> Option<String>) -> Option<Ipv4Addr> {
    if !on {
        return None;
    }
    tailscale_ip(&ask()?)
}

/// What `tailscale ip -4` prints, if it is installed and connected.
pub fn ask_tailscale() -> Option<String> {
    let out = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_tailscale_address_is_one() {
        assert_eq!(
            tailscale_ip("100.101.102.103\n"),
            Some(Ipv4Addr::new(100, 101, 102, 103))
        );
        assert_eq!(
            tailscale_ip("100.64.0.1"),
            Some(Ipv4Addr::new(100, 64, 0, 1))
        );
        assert_eq!(
            tailscale_ip("100.127.255.254\nfd7a::1\n"),
            Some(Ipv4Addr::new(100, 127, 255, 254))
        );
        for not_one in [
            "192.168.1.20",
            "100.63.255.255",
            "100.128.0.1",
            "8.8.8.8",
            "127.0.0.1",
            "0.0.0.0",
            "",
            "Tailscale is stopped.",
        ] {
            assert_eq!(tailscale_ip(not_one), None, "{not_one}");
        }
    }

    #[test]
    fn it_shares_only_when_turned_on_and_tailscale_says_where() {
        let says = || Some("100.101.102.103\n".to_string());
        assert_eq!(
            address(true, &says),
            Some(Ipv4Addr::new(100, 101, 102, 103))
        );
        assert_eq!(
            address(false, &|| panic!("asked Tailscale while off")),
            None
        );
        assert_eq!(address(true, &|| None), None, "not installed");
        assert_eq!(
            address(true, &|| Some("192.168.1.20".into())),
            None,
            "not its address"
        );
    }
}
