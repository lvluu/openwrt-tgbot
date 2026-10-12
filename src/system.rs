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

pub fn online_ips() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(s) = std::fs::read_to_string("/proc/net/arp") {
        for (i, line) in s.lines().enumerate() {
            if i == 0 || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 6 && f[2] == "0x2" {
                v.push(f[0].to_string());
            }
        }
    }
    v
}

pub fn parse_arp(arp: &str, want_mac_lower: &str) -> Option<String> {
    for (i, line) in arp.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 6 && f[2] == "0x2" && f[3].to_lowercase() == want_mac_lower {
            return Some(f[0].to_string());
        }
    }
    None
}

pub fn online_ip_by_mac(mac: &str) -> Option<String> {
    let arp = std::fs::read_to_string("/proc/net/arp").unwrap_or_default();
    parse_arp(&arp, &mac.to_lowercase())
}

pub fn is_online(ip: &str) -> bool {
    online_ips().iter().any(|x| x == ip)
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

    #[test]
    fn parse_arp_finds_complete_entry_only() {
        let arp =
            "IP address       HW type     Flags       HW address            Mask     Device\n\
                   192.168.1.141   0x1         0x2         02:00:00:00:00:10     *        br-lan\n\
                   192.168.1.142   0x1         0x0         aa:bb:cc:dd:ee:ff     *        br-lan\n";
        assert_eq!(
            parse_arp(arp, "02:00:00:00:00:10"),
            Some("192.168.1.141".to_string())
        );
        assert_eq!(parse_arp(arp, "AA:BB:CC:DD:EE:FF"), None);
        assert_eq!(parse_arp("", "02:00:00:00:00:10"), None);
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
