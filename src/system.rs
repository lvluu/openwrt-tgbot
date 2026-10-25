use serde_json::Value;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::Command;

pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        if part.len() != 2 {
            return None;
        }
        out[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(out)
}

/// Directed broadcast of the first IPv4 address in a netifd `status` JSON
/// (`ipv4-address[0]` = `{"address": "192.168.1.1", "mask": 24}`).
pub fn lan_broadcast_from_status(json: &str) -> Option<Ipv4Addr> {
    let v: Value = serde_json::from_str(json).ok()?;
    let entry = &v["ipv4-address"][0];
    let addr: Ipv4Addr = entry["address"].as_str()?.parse().ok()?;
    let mask = u32::try_from(entry["mask"].as_u64()?)
        .ok()
        .filter(|m| *m <= 30)?;
    let host_bits = u32::MAX >> mask;
    Some(Ipv4Addr::from(u32::from(addr) | host_bits))
}

/// Broadcast address of the LAN interface, or `None` when netifd cannot tell.
pub fn lan_broadcast() -> Option<Ipv4Addr> {
    lan_broadcast_from_status(&sh("ubus call network.interface.lan status"))
}

/// Configured addresses plus the detected LAN broadcast, deduplicated, in that order.
pub fn wake_targets(configured: &[Ipv4Addr], detected: Option<Ipv4Addr>) -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    for a in configured.iter().copied().chain(detected) {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// Sends the magic packet to each directed broadcast. The unbound limited broadcast
/// (255.255.255.255) is never used: on a router it follows the default route, not br-lan.
pub fn wake_all(mac: &str, bcasts: &[Ipv4Addr]) -> Result<usize, String> {
    let bytes = parse_mac(mac).ok_or_else(|| format!("bad mac address: {mac}"))?;
    let targets = wake_targets(bcasts, lan_broadcast());
    if targets.is_empty() {
        return Err(
            "no LAN broadcast address found; set wake_bcast=<addr> in /etc/tgbot.conf".to_string(),
        );
    }
    let pkt = wake_on_lan::MagicPacket::new(&bytes);
    let from = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
    let mut sent = 0usize;
    let mut last_err = String::new();
    for t in targets {
        match pkt.send_to(SocketAddr::from((t, 9)), from) {
            Ok(()) => sent += 1,
            Err(e) => {
                eprintln!("tgbot: wake send to {t} failed: {e}");
                last_err = format!("{t}: {e}");
            }
        }
    }
    if sent == 0 {
        Err(format!("magic packet send failed ({last_err})"))
    } else {
        Ok(sent)
    }
}

pub fn sh(cmd: &str) -> String {
    match Command::new("/bin/sh").arg("-c").arg(cmd).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

pub fn read_trim(p: &str) -> String {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn kb_suffix(prefix: &str, line: &str) -> u64 {
    line.strip_prefix(prefix)
        .and_then(|v| v.split_whitespace().next())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

pub fn uptime_str() -> String {
    let up: f64 = read_trim("/proc/uptime")
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let d = (up / 86400.0) as u64;
    let h = ((up % 86400.0) / 3600.0) as u64;
    let m = ((up % 3600.0) / 60.0) as u64;
    if d > 0 {
        format!("{}d {}h {}m", d, h, m)
    } else {
        format!("{}h {}m", h, m)
    }
}

pub fn mem_str() -> String {
    let mut total = 0u64;
    let mut avail = 0u64;
    if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
        for line in s.lines() {
            total = kb_suffix("MemTotal:", line).max(total);
            avail = kb_suffix("MemAvailable:", line).max(avail);
        }
    }
    format!("{} of {} MB free", avail / 1024, total / 1024)
}

pub fn temp_str() -> String {
    match std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        Ok(s) => match s.trim().parse::<f64>() {
            Ok(t) => format!("{:.0} °C", t / 1000.0),
            Err(_) => "n/a".to_string(),
        },
        Err(_) => "n/a".to_string(),
    }
}

pub fn wan_ip() -> String {
    let out = sh("ubus call network.interface.wan status");
    if let Ok(v) = serde_json::from_str::<Value>(&out) {
        if let Some(a) = v["ipv4-address"][0]["address"].as_str() {
            return a.to_string();
        }
    }
    "n/a".to_string()
}

/// Resolved ARP entries as (ip, mac). Flag 0x2 is set for every valid
/// neighbour state, STALE included, so this is only a candidate list.
pub fn arp_entries(arp: &str) -> Vec<(String, String)> {
    arp.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            (f.len() >= 6 && f[2] == "0x2").then(|| (f[0].to_string(), f[3].to_lowercase()))
        })
        .collect()
}

/// IPv4 neighbours the kernel confirmed reachable (`ip neigh` state REACHABLE).
pub fn reachable_ips(neigh: &str) -> Vec<String> {
    neigh
        .lines()
        .filter_map(|line| {
            let mut t = line.split_whitespace();
            let ip = t.next()?;
            ip.parse::<std::net::Ipv4Addr>().ok()?;
            (line.split_whitespace().last() == Some("REACHABLE")).then(|| ip.to_string())
        })
        .collect()
}

/// Cap on concurrent pings: low-RAM router, no async runtime.
const MAX_PARALLEL_PROBES: usize = 8;

/// Online = confirmed REACHABLE, or answers an active probe. Pure so the
/// state handling is testable; `probe` is only called for unconfirmed entries.
pub fn confirmed_online(
    entries: &[(String, String)],
    reachable: &[String],
    probe: impl Fn(&str) -> bool + Sync,
) -> Vec<(String, String)> {
    let mut up = Vec::new();
    let mut unconfirmed = Vec::new();
    for e in entries {
        if reachable.contains(&e.0) {
            up.push(e.clone());
        } else {
            unconfirmed.push(e);
        }
    }
    for batch in unconfirmed.chunks(MAX_PARALLEL_PROBES) {
        std::thread::scope(|sc| {
            let probe = &probe;
            let handles: Vec<_> = batch
                .iter()
                .map(|&e| {
                    std::thread::Builder::new()
                        .spawn_scoped(sc, move || probe(&e.0))
                        .ok()
                })
                .collect();
            for (&e, h) in batch.iter().zip(handles) {
                if h.is_some_and(|h| h.join().unwrap_or(false)) {
                    up.push(e.clone());
                }
            }
        });
    }
    up
}

fn live_entries(only: impl Fn(&(String, String)) -> bool) -> Vec<(String, String)> {
    let arp = std::fs::read_to_string("/proc/net/arp").unwrap_or_default();
    let entries: Vec<_> = arp_entries(&arp).into_iter().filter(only).collect();
    let reachable = reachable_ips(&sh("ip neigh show"));
    confirmed_online(&entries, &reachable, ping)
}

/// One ARP/neigh read and one parallel probe round for a set of IPs.
pub fn online_among(ips: &[String]) -> Vec<String> {
    live_entries(|e| ips.contains(&e.0))
        .into_iter()
        .map(|e| e.0)
        .collect()
}

pub fn online_ips() -> Vec<String> {
    live_entries(|_| true).into_iter().map(|e| e.0).collect()
}

pub fn online_ip_by_mac(mac: &str) -> Option<String> {
    let mac = mac.to_lowercase();
    live_entries(|e| e.1 == mac).into_iter().next().map(|e| e.0)
}

pub fn ping(ip: &str) -> bool {
    Command::new("ping")
        .args(["-c", "1", "-W", "2", ip])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn reboot() {
    sh("sync; sleep 1; reboot");
}

#[cfg(test)]
mod tests {
    const LAN_STATUS: &str = r#"{"up":true,"ipv4-address":[{"address":"192.168.1.1","mask":24}]}"#;

    #[test]
    fn lan_broadcast_is_derived_from_address_and_mask() {
        assert_eq!(
            lan_broadcast_from_status(LAN_STATUS),
            Some(Ipv4Addr::new(192, 168, 1, 255))
        );
        let wide = r#"{"ipv4-address":[{"address":"10.1.2.3","mask":22}]}"#;
        assert_eq!(
            lan_broadcast_from_status(wide),
            Some(Ipv4Addr::new(10, 1, 3, 255))
        );
    }

    #[test]
    fn lan_broadcast_none_without_usable_address() {
        assert_eq!(lan_broadcast_from_status(""), None);
        assert_eq!(lan_broadcast_from_status(r#"{"up":false}"#), None);
        let p2p = r#"{"ipv4-address":[{"address":"10.0.0.1","mask":32}]}"#;
        assert_eq!(lan_broadcast_from_status(p2p), None);
    }

    #[test]
    fn wake_targets_dedupes_and_never_uses_limited_broadcast() {
        let a = Ipv4Addr::new(192, 168, 1, 255);
        let b = Ipv4Addr::new(192, 168, 2, 255);
        assert_eq!(wake_targets(&[a, b], Some(a)), vec![a, b]);
        assert_eq!(wake_targets(&[b], Some(a)), vec![b, a]);
        assert!(wake_targets(&[], None).is_empty());
    }

    use super::*;

    #[test]
    fn parse_mac_accepts_colon_and_dash() {
        assert_eq!(
            parse_mac("02:00:00:00:00:01"),
            Some([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])
        );
        assert_eq!(
            parse_mac("02-00-00-00-00-03"),
            Some([0x02, 0x00, 0x00, 0x00, 0x00, 0x03])
        );
    }

    #[test]
    fn parse_mac_rejects_junk() {
        assert_eq!(parse_mac("02:00:00:00:00"), None);
        assert_eq!(parse_mac("02:00:00:00:00:0G"), None);
        assert_eq!(parse_mac(""), None);
    }

    const ARP: &str =
        "IP address       HW type     Flags       HW address            Mask     Device\n\
         192.168.1.141   0x1         0x2         02:00:00:00:00:10     *        br-lan\n\
         192.168.1.142   0x1         0x0         aa:bb:cc:dd:ee:ff     *        br-lan\n\
         192.168.1.143   0x1         0x2         02:00:00:00:00:13     *        br-lan\n";

    const NEIGH: &str = "192.168.1.141 dev br-lan lladdr 02:00:00:00:00:10 REACHABLE\n\
         192.168.1.143 dev br-lan lladdr 02:00:00:00:00:13 STALE\n\
         192.168.1.144 dev br-lan lladdr 02:00:00:00:00:14 router REACHABLE\n\
         192.168.1.145 dev br-lan lladdr 02:00:00:00:00:15 DELAY\n\
         192.168.1.146 dev br-lan FAILED\n\
         192.168.1.147 dev br-lan INCOMPLETE\n\
         fe80::1 dev br-lan lladdr 02:00:00:00:00:99 REACHABLE\n";

    #[test]
    fn arp_entries_keep_resolved_only() {
        assert_eq!(
            arp_entries(ARP),
            vec![
                ("192.168.1.141".to_string(), "02:00:00:00:00:10".to_string()),
                ("192.168.1.143".to_string(), "02:00:00:00:00:13".to_string()),
            ]
        );
        assert!(arp_entries("").is_empty());
    }

    #[test]
    fn reachable_ips_only_counts_reachable_ipv4() {
        assert_eq!(
            reachable_ips(NEIGH),
            vec!["192.168.1.141".to_string(), "192.168.1.144".to_string()]
        );
        assert!(reachable_ips("").is_empty());
    }

    #[test]
    fn stale_entry_needs_a_successful_probe() {
        let entries = arp_entries(ARP);
        let reachable = reachable_ips(NEIGH);
        let dead = confirmed_online(&entries, &reachable, |_| false);
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].0, "192.168.1.141");
        let alive = confirmed_online(&entries, &reachable, |ip| ip == "192.168.1.143");
        assert_eq!(alive.len(), 2);
    }

    #[test]
    fn reachable_entry_is_not_probed() {
        let entries = arp_entries(ARP);
        let up = confirmed_online(&entries, &["192.168.1.141".to_string()], |ip| {
            assert_ne!(ip, "192.168.1.141", "confirmed entry must not be probed");
            false
        });
        assert_eq!(up.len(), 1);
    }

    #[test]
    fn wake_all_validates_before_sending() {
        assert!(wake_all("nope", &[]).is_err());
        assert!(wake_all("02:00:00:00", &[]).is_err());
        assert_eq!(wake_all("02:00:00:00:00:01", &[Ipv4Addr::LOCALHOST]), Ok(1));
    }

    #[test]
    fn magic_packet_layout() {
        let mac = parse_mac("02:00:00:00:00:03").unwrap();
        let pkt = wake_on_lan::MagicPacket::new(&mac);
        let bytes = pkt.magic_bytes();
        assert_eq!(bytes.len(), 102);
        assert!(bytes[..6].iter().all(|&b| b == 0xFF));
        assert!(bytes[6..].chunks(6).all(|c| c == mac));
    }
}
