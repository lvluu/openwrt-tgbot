use serde_json::Value;
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

pub fn wake_all(mac: &str, bcasts: &[String]) -> Result<usize, String> {
    let bytes = parse_mac(mac).ok_or_else(|| format!("bad mac address: {mac}"))?;
    let pkt = wake_on_lan::MagicPacket::new(&bytes);
    let mut sent = 0usize;
    if pkt.send().is_ok() {
        sent += 1;
    }
    for b in bcasts {
        let addr: std::net::SocketAddr = match format!("{b}:9").parse() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let from: std::net::SocketAddr = "0.0.0.0:0".parse().expect("bind any");
        if pkt.send_to(addr, from).is_ok() {
            sent += 1;
        }
    }
    if sent == 0 {
        Err("no magic packet could be sent".to_string())
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

/// Online = confirmed REACHABLE, or answers an active probe. Pure so the
/// state handling is testable; `probe` is only called for unconfirmed entries.
pub fn confirmed_online(
    entries: &[(String, String)],
    reachable: &[String],
    probe: impl Fn(&str) -> bool + Sync,
) -> Vec<(String, String)> {
    std::thread::scope(|sc| {
        let probe = &probe;
        let handles: Vec<_> = entries
            .iter()
            .map(|e| {
                let confirmed = reachable.contains(&e.0);
                sc.spawn(move || confirmed || probe(&e.0))
            })
            .collect();
        entries
            .iter()
            .zip(handles)
            .filter_map(|(e, h)| h.join().unwrap_or(false).then(|| e.clone()))
            .collect()
    })
}

fn live_entries(only: impl Fn(&(String, String)) -> bool) -> Vec<(String, String)> {
    let arp = std::fs::read_to_string("/proc/net/arp").unwrap_or_default();
    let entries: Vec<_> = arp_entries(&arp).into_iter().filter(only).collect();
    let reachable = reachable_ips(&sh("ip neigh show"));
    confirmed_online(&entries, &reachable, ping)
}

pub fn online_ips() -> Vec<String> {
    live_entries(|_| true).into_iter().map(|e| e.0).collect()
}

pub fn online_ip_by_mac(mac: &str) -> Option<String> {
    let mac = mac.to_lowercase();
    live_entries(|e| e.1 == mac).into_iter().next().map(|e| e.0)
}

pub fn is_online(ip: &str) -> bool {
    !live_entries(|e| e.0 == ip).is_empty()
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
        assert!(wake_all("02:00:00:00:00:01", &[]).is_ok());
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
