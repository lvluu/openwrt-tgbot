use crate::config::Config;
use crate::system;
use crate::telegram;

const DEADLINE_SECS: u64 = 180;
const POLL_SECS: u64 = 5;

pub fn resolve_target(input: &str, wakes: &[(String, String)]) -> Option<(String, String)> {
    let needle = input.to_lowercase();
    wakes
        .iter()
        .find(|(mac, name)| name.to_lowercase() == needle || mac.to_lowercase() == needle)
        .cloned()
}

pub fn up_message(name: &str, ip: &str, elapsed: u64) -> String {
    if elapsed <= POLL_SECS {
        format!(
            "⚡ <b>{}</b> is online at <code>{}</code>",
            telegram::esc(name),
            ip
        )
    } else {
        format!(
            "⚡ <b>{}</b> is up at <code>{}</code> after {}s",
            telegram::esc(name),
            ip,
            elapsed
        )
    }
}

pub fn timeout_message(name: &str, deadline: u64) -> String {
    format!(
        "😴 No sign of <b>{}</b> {}s after the wake packet",
        telegram::esc(name),
        deadline
    )
}

pub fn watch(cfg: Config, mac: String, name: String) {
    let mut elapsed = 0u64;
    while elapsed < DEADLINE_SECS {
        std::thread::sleep(std::time::Duration::from_secs(POLL_SECS));
        elapsed += POLL_SECS;
        if let Some(ip) = system::online_ip_by_mac(&mac) {
            telegram::send(&cfg, &up_message(&name, &ip, elapsed));
            return;
        }
    }
    telegram::send(&cfg, &timeout_message(&name, DEADLINE_SECS));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_by_name_or_mac() {
        let wakes = vec![("02:00:00:00:00:01".to_string(), "node1".to_string())];
        assert_eq!(
            resolve_target("node1", &wakes),
            Some(("02:00:00:00:00:01".to_string(), "node1".to_string()))
        );
        assert_eq!(
            resolve_target("NODE1", &wakes),
            Some(("02:00:00:00:00:01".to_string(), "node1".to_string()))
        );
        assert_eq!(
            resolve_target("02:00:00:00:00:01", &wakes),
            Some(("02:00:00:00:00:01".to_string(), "node1".to_string()))
        );
        assert_eq!(resolve_target("node9", &wakes), None);
    }

    #[test]
    fn up_message_mentions_wait_time() {
        assert!(up_message("node1", "192.168.1.55", 45).contains("after 45s"));
        assert!(up_message("node1", "192.168.1.55", 2).contains("is online at"));
    }

    #[test]
    fn timeout_message_mentions_deadline() {
        assert!(timeout_message("node1", 180).contains("180s"));
    }
}
