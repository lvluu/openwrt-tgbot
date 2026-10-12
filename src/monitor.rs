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

pub fn run(cfg: Config) {
    if cfg.chat_id == 0 || cfg.monitors.is_empty() {
        return;
    }
    let mut state: Vec<bool> = vec![false; cfg.monitors.len()];
    let mut first = true;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(20));
        for (i, (ip, name)) in cfg.monitors.iter().enumerate() {
            let on = system::is_online(ip);
            if !first && on != state[i] {
                telegram::send(&cfg, &presence_message(name, ip, on));
            }
            state[i] = on;
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
    fn presence_message_escapes_device_names() {
        let msg = presence_message("<script>", "1.2.3.4", true);
        assert!(msg.contains("&lt;script&gt;"));
        assert!(!msg.contains("<script>"));
    }
}
