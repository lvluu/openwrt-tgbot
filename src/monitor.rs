use crate::config::Config;
use crate::system;
use crate::telegram;

pub fn presence_message(name: &str, ip: &str, online: bool) -> String {
    if online {
        format!(
            "\u{1f7e2} <b>{}</b> came online ({})",
            telegram::esc(name),
            ip
        )
    } else {
        format!(
            "\u{1f534} <b>{}</b> went offline ({})",
            telegram::esc(name),
            ip
        )
    }
}

/// Misses in a row before a device counts as offline; absorbs a single
/// dropped probe from hosts that idle without answering ICMP.
const OFFLINE_AFTER_MISSES: u32 = 2;

/// Next (reported_online, miss_count) given a fresh probe result.
pub fn next_presence(reported: bool, misses: u32, seen: bool) -> (bool, u32) {
    if seen {
        (true, 0)
    } else if reported && misses + 1 < OFFLINE_AFTER_MISSES {
        (true, misses + 1)
    } else {
        (false, misses + 1)
    }
}

pub fn run(cfg: Config) {
    if cfg.chat_id == 0 || cfg.monitors.is_empty() {
        return;
    }
    let mut state: Vec<bool> = vec![false; cfg.monitors.len()];
    let mut misses: Vec<u32> = vec![0; cfg.monitors.len()];
    let ips: Vec<String> = cfg.monitors.iter().map(|(ip, _)| ip.clone()).collect();
    let mut first = true;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(20));
        let seen_ips = system::online_among(&ips);
        for (i, (ip, name)) in cfg.monitors.iter().enumerate() {
            let seen = seen_ips.contains(ip);
            let (on, m) = next_presence(state[i], misses[i], seen);
            if !first && on != state[i] {
                telegram::send(&cfg, &presence_message(name, ip, on));
            }
            state[i] = on;
            misses[i] = m;
        }
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_message_covers_both_transitions() {
        assert_eq!(
            presence_message("android-tv", "192.168.1.141", true),
            "\u{1f7e2} <b>android-tv</b> came online (192.168.1.141)"
        );
        assert_eq!(
            presence_message("android-tv", "192.168.1.141", false),
            "\u{1f534} <b>android-tv</b> went offline (192.168.1.141)"
        );
    }

    #[test]
    fn offline_needs_two_misses_in_a_row() {
        assert_eq!(next_presence(true, 0, false), (true, 1));
        assert_eq!(next_presence(true, 1, false), (false, 2));
        assert_eq!(next_presence(true, 1, true), (true, 0));
        assert_eq!(next_presence(false, 5, false), (false, 6));
        assert_eq!(next_presence(false, 5, true), (true, 0));
    }

    #[test]
    fn presence_message_escapes_device_names() {
        let msg = presence_message("<script>", "1.2.3.4", true);
        assert!(msg.contains("&lt;script&gt;"));
        assert!(!msg.contains("<script>"));
    }
}
