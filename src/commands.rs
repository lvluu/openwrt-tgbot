use crate::config::Config;
use crate::system;
use crate::telegram;
use serde_json::{json, Value};
use tabled::builder::Builder;
use tabled::settings::Style;

pub const HELP: &str = "<b>Router bot</b>
/status - router health (uptime, load, mem, temp, WAN)
/clients - DHCP leases with online state
/ping &lt;ip&gt; - ping a device on the LAN
/tailscale - tailscale netmap
/wake - click a machine to wake it (Wake-on-LAN)
/watch &lt;machine&gt; - watch a machine come online
/log [n] - last n log lines (5-60)
/restartwifi - restart networking
/reboot - reboot router (with confirm)";

pub fn status_report() -> String {
    let load_raw = system::read_trim("/proc/loadavg");
    let load: Vec<&str> = load_raw.split_whitespace().take(3).collect();
    format!(
        "🏠 <b>Router</b>\nUptime: {}\nLoad: {}\nMem: {}\nTemp: {}\nWAN: {}\nDevices online: {}",
        system::uptime_str(),
        load.join(" "),
        system::mem_str(),
        system::temp_str(),
        system::wan_ip(),
        system::online_ips().len()
    )
}

pub fn leases_table(leases: &str, online: &[String]) -> String {
    let mut b = Builder::default();
    b.push_record(["", "IP", "Name"]);
    for line in leases.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 4 {
            let ip = f[2];
            let name = if f[3] == "*" { "unknown" } else { f[3] };
            let dot = if online.iter().any(|i| i == ip) {
                "🟢"
            } else {
                "⚪"
            };
            b.push_record([dot, ip, &telegram::esc(name)]);
        }
    }
    preformatted(&b.build().with(Style::rounded()).to_string())
}

pub fn clients_report() -> String {
    let leases = std::fs::read_to_string("/tmp/dhcp.leases").unwrap_or_default();
    leases_table(&leases, &system::online_ips())
}

pub fn ping_report(ip: &str, online: bool) -> String {
    if online {
        format!("📶 <code>{}</code> is <b>online</b>", ip)
    } else {
        format!("🔇 <code>{}</code> is <b>offline / unreachable</b>", ip)
    }
}

pub fn wake_report(name: &str, res: &Result<usize, String>) -> String {
    match res {
        Ok(n) => format!(
            "⚡ Magic packet sent to <b>{}</b> ({} target{})",
            telegram::esc(name),
            n,
            if *n == 1 { "" } else { "s" }
        ),
        Err(e) => format!("⚠️ Could not wake <b>{}</b>: {}", telegram::esc(name), e),
    }
}

fn wake_keyboard(cfg: &Config) -> Value {
    let rows: Vec<Value> = cfg
        .wakes
        .iter()
        .map(|(mac, name)| {
            json!([{
                "text": name,
                "callback_data": format!("wake:{}", mac),
            }])
        })
        .collect();
    json!({"inline_keyboard": rows})
}

pub fn wake_prompt(cfg: &Config) {
    if cfg.wakes.is_empty() {
        telegram::send(
            cfg,
            "No wake targets configured. Add <code>wake=MAC:name</code> lines to /etc/tgbot.conf.",
        );
        return;
    }
    let payload = json!({
        "chat_id": cfg.chat_id,
        "text": "Wake which machine?",
        "reply_markup": wake_keyboard(cfg),
    })
    .to_string();
    let _ = telegram::api(cfg, "sendMessage", &payload);
}

const MAX_MESSAGE_CHARS: usize = 4096;
const EMPTY_LOG: &str = "(log is empty)";

/// Escapes log output into a `<pre>` block, dropping the oldest lines so it fits one message.
pub fn log_report(raw: &str) -> String {
    if raw.trim().is_empty() {
        return EMPTY_LOG.to_string();
    }
    let budget = MAX_MESSAGE_CHARS - "<pre></pre>".len();
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0;
    for line in raw.lines().rev() {
        let mut esc = telegram::esc(line);
        let mut cost = esc.chars().count() + usize::from(!kept.is_empty());
        if kept.is_empty() && cost > budget {
            // one line alone is too long: keep its tail; escaping grows a char at most 5x
            let max_raw = budget / 5;
            let skip = line.chars().count().saturating_sub(max_raw);
            esc = telegram::esc(&line.chars().skip(skip).collect::<String>());
            cost = esc.chars().count();
        }
        if used + cost > budget {
            break;
        }
        used += cost;
        kept.push(esc);
    }
    kept.reverse();
    format!("<pre>{}</pre>", kept.join("\n"))
}

fn preformatted(body: &str) -> String {
    format!("<pre>{}</pre>", telegram::esc(body))
}

pub fn format_netmap(status: &str) -> String {
    let mut b = Builder::default();
    b.push_record(["IP", "Node", "User", "OS"]);
    let mut count = 0;
    for line in status.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 4 {
            b.push_record([f[0], f[1], f[2], f[3]]);
            count += 1;
        }
    }
    if count == 0 {
        return "tailscale: no data".to_string();
    }
    preformatted(&b.build().with(Style::rounded()).to_string())
}

pub fn tailscale_report() -> String {
    format_netmap(&system::sh("tailscale status"))
}

pub fn reboot_prompt(cfg: &Config) {
    let payload = json!({
        "chat_id": cfg.chat_id,
        "text": "Reboot the router?",
        "reply_markup": {
            "inline_keyboard": [[
                {"text": "Yes, reboot", "callback_data": "reboot_yes"},
                {"text": "Cancel", "callback_data": "reboot_no"}
            ]]
        }
    })
    .to_string();
    let _ = telegram::api(cfg, "sendMessage", &payload);
}

pub fn handle_message(cfg: &mut Config, chat: i64, text: &str) {
    if cfg.chat_id == 0 {
        if let Some(rest) = text.strip_prefix("/start ") {
            if !cfg.pair_code.is_empty() && rest.trim() == cfg.pair_code {
                cfg.chat_id = chat;
                cfg.persist_chat_id();
                telegram::send(cfg, "✅ Paired. bot is online.");
                telegram::send(cfg, HELP);
            }
        }
        return;
    }
    if chat != cfg.chat_id {
        return;
    }
    let parts: Vec<&str> = text.split_whitespace().collect();
    let cmd = parts[0].split('@').next().unwrap_or("").to_lowercase();
    match cmd.as_str() {
        "/start" | "/help" => telegram::send(cfg, HELP),
        "/status" => telegram::send(cfg, &status_report()),
        "/clients" => telegram::send(cfg, &clients_report()),
        "/ping" => match parts.get(1).map(|s| s.parse::<std::net::IpAddr>()) {
            Some(Ok(ip)) => {
                let ip = ip.to_string();
                telegram::send(cfg, &ping_report(&ip, system::ping(&ip)));
            }
            _ => telegram::send(cfg, "Usage: /ping &lt;ip&gt;"),
        },
        "/tailscale" => telegram::send(cfg, &tailscale_report()),
        "/log" => {
            let n = parts
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(15)
                .clamp(5, 60);
            telegram::send(
                cfg,
                &log_report(&system::sh(&format!("logread | tail -n {}", n))),
            );
        }
        "/restartwifi" => {
            telegram::send(cfg, "🔄 Restarting network...");
            system::sh("/etc/init.d/network restart");
        }
        "/wake" => wake_prompt(cfg),
        "/watch" => {
            let target = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
            match crate::watcher::resolve_target(&target, &cfg.wakes) {
                Some((mac, name)) => {
                    telegram::send(
                        cfg,
                        &format!(
                            "👀 Watching <b>{}</b> for the next 3 minutes...",
                            telegram::esc(&name)
                        ),
                    );
                    let wcfg = cfg.clone();
                    std::thread::spawn(move || crate::watcher::watch(wcfg, mac, name));
                }
                None => telegram::send(cfg, "Unknown machine. Send /wake to see the list."),
            }
        }
        "/reboot" => reboot_prompt(cfg),
        _ => telegram::send(cfg, "Unknown command. Try /help"),
    }
}

pub fn handle_callback(cfg: &Config, cb: &Value) {
    let data = cb["data"].as_str().unwrap_or("").to_string();
    let chat = cb["message"]["chat"]["id"].as_i64().unwrap_or(0);
    let mid = cb["message"]["message_id"].as_i64().unwrap_or(0);
    let cbid = cb["id"].as_str().unwrap_or("").to_string();
    if chat != cfg.chat_id || cbid.is_empty() {
        return;
    }
    if let Some(mac) = data.strip_prefix("wake:") {
        let name = cfg
            .wakes
            .iter()
            .find(|(m, _)| m == mac)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| mac.to_string());
        let ack = json!({"callback_query_id": cbid, "text": "sending magic packet"}).to_string();
        let _ = telegram::api(cfg, "answerCallbackQuery", &ack);
        let res = system::wake_all(mac, &cfg.wake_bcasts);
        let text = wake_report(&name, &res);
        if res.is_ok() {
            let wcfg = cfg.clone();
            let (mac_owned, name_owned) = (mac.to_string(), name.clone());
            std::thread::spawn(move || crate::watcher::watch(wcfg, mac_owned, name_owned));
        }
        let edit = json!({
            "chat_id": chat,
            "message_id": mid,
            "text": text,
            "parse_mode": "HTML",
        })
        .to_string();
        let _ = telegram::api(cfg, "editMessageText", &edit);
        return;
    }
    if data == "reboot_yes" {
        let ack = json!({"callback_query_id": cbid, "text": "rebooting"}).to_string();
        let _ = telegram::api(cfg, "answerCallbackQuery", &ack);
        telegram::send(cfg, "🔌 Rebooting now. Back in about a minute.");
        system::reboot();
    } else {
        let ack = json!({"callback_query_id": cbid, "text": "cancelled"}).to_string();
        let _ = telegram::api(cfg, "answerCallbackQuery", &ack);
        let edit =
            json!({"chat_id": chat, "message_id": mid, "text": "Reboot cancelled."}).to_string();
        let _ = telegram::api(cfg, "editMessageText", &edit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_report_escapes_markup() {
        assert_eq!(log_report("a <b> & c"), "<pre>a &lt;b&gt; &amp; c</pre>");
    }

    #[test]
    fn log_report_placeholder_when_empty() {
        assert_eq!(log_report(""), EMPTY_LOG);
        assert_eq!(log_report("\n  \n"), EMPTY_LOG);
    }

    #[test]
    fn log_report_trims_oldest_lines_to_fit() {
        let raw: String = (0..200)
            .map(|i| format!("line {:03} {}\n", i, "x".repeat(40)))
            .collect();
        let out = log_report(&raw);
        assert!(out.chars().count() <= MAX_MESSAGE_CHARS);
        assert!(out.contains("line 199"));
        assert!(!out.contains("line 000"));
    }

    #[test]
    fn log_report_oversize_single_line_stays_within_limit() {
        let out = log_report(&"<&".repeat(5000));
        assert!(out.chars().count() <= MAX_MESSAGE_CHARS);
        assert!(out.starts_with("<pre>") && out.ends_with("</pre>"));
    }

    #[test]
    fn wake_buttons_keep_raw_names() {
        let cfg = Config {
            token: String::new(),
            chat_id: 0,
            pair_code: String::new(),
            monitors: vec![],
            wakes: vec![("AA:BB:CC:DD:EE:FF".into(), "A&B".into())],
            wake_bcasts: vec![],
            path: String::new(),
        };
        let kb = wake_keyboard(&cfg);
        assert_eq!(kb["inline_keyboard"][0][0]["text"], "A&B");
    }

    fn table(out: &str) -> String {
        out.lines()
            .map(|l| l.strip_prefix("<pre>").unwrap_or(l))
            .map(|l| l.strip_suffix("</pre>").unwrap_or(l))
            .collect::<Vec<&str>>()
            .join("\n")
    }

    fn line_widths(out: &str) -> Vec<usize> {
        let t = table(out);
        t.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.chars().count())
            .collect()
    }

    #[test]
    fn netmap_renders_aligned_table() {
        let input = "100.64.0.1  hostname  user@  os  -\n\
                     100.64.0.2   laptop   alice@  linux  -\n\
                     100.64.0.3   phone      alice@  android  -\n";
        let out = format_netmap(input);
        assert!(out.starts_with("<pre>") && out.ends_with("</pre>"));
        assert!(out.contains("100.64.0.1"));
        assert!(out.contains("laptop"));
        assert!(out.contains("phone"));
        let w = line_widths(&out);
        assert!(w.iter().all(|&x| x == w[0]), "every row equal width: {w:?}");
    }

    #[test]
    fn leases_table_marks_online_and_offline() {
        let leases = "1791641454 02:00:00:00:00:10 192.168.1.141 androidtv 01:02:00\n\
                      1791640900 aa:bb:cc:dd:ee:01 192.168.1.155 * 02:aa:bb\n";
        let online = vec!["192.168.1.141".to_string()];
        let out = leases_table(leases, &online);
        assert!(out.contains("\u{1f7e2}"));
        assert!(out.contains("\u{26aa}"));
        assert!(out.contains("192.168.1.141"));
        assert!(out.contains("unknown"));
        let table = out.replace("<pre>", "").replace("</pre>", "");
        let widths: Vec<usize> = table
            .lines()
            .map(unicode_width::UnicodeWidthStr::width)
            .collect();
        assert!(
            widths.iter().all(|&w| w == widths[0]),
            "rows equal width: {widths:?}"
        );
    }

    #[test]
    fn ping_report_formats_both_states() {
        assert!(ping_report("192.168.1.141", true).contains("online"));
        assert!(!ping_report("192.168.1.141", true).contains("offline"));
        assert!(ping_report("192.168.1.141", false).contains("offline"));
    }

    #[test]
    fn wake_report_formats_success_and_failure() {
        let ok = wake_report("node1", &Ok(2));
        assert!(ok.contains("node1") && ok.contains("2 targets"));
        let err = wake_report("node1", &Err("bad mac address: 10".to_string()));
        assert!(err.contains("Could not wake") && err.contains("bad mac address: 10"));
    }

    #[test]
    fn status_report_has_core_fields() {
        let s = status_report();
        assert!(s.contains("Uptime:"));
        assert!(s.contains("Mem:"));
        assert!(s.contains("WAN:"));
        assert!(s.contains("Devices online:"));
    }

    #[test]
    fn wake_keyboard_has_one_button_per_machine() {
        let mut cfg = crate::config::Config::load("/nonexistent");
        cfg.wakes = vec![
            ("02:00:00:00:00:01".to_string(), "node1".to_string()),
            ("02:00:00:00:00:03".to_string(), "node3".to_string()),
        ];
        let kb = wake_keyboard(&cfg);
        let buttons = kb["inline_keyboard"].as_array().unwrap();
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0][0]["text"], "node1");
        assert_eq!(buttons[0][0]["callback_data"], "wake:02:00:00:00:00:01");
        assert_eq!(buttons[1][0]["text"], "node3");
    }

    #[test]
    fn netmap_handles_empty_input() {
        assert_eq!(format_netmap(""), "tailscale: no data");
        assert_eq!(format_netmap("   \n"), "tailscale: no data");
    }
}
