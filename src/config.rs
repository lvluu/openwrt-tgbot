#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub chat_id: i64,
    pub pair_code: String,
    pub monitors: Vec<(String, String)>,
    pub wakes: Vec<(String, String)>,
    pub wake_bcasts: Vec<String>,
    pub path: String,
}

impl Config {
    pub fn load(path: &str) -> Config {
        let mut cfg = Config {
            token: String::new(),
            chat_id: 0,
            pair_code: String::new(),
            monitors: Vec::new(),
            wakes: Vec::new(),
            wake_bcasts: Vec::new(),
            path: path.to_string(),
        };
        if let Ok(s) = std::fs::read_to_string(path) {
            for line in s.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let (k, v) = (k.trim(), v.trim());
                    match k {
                        "token" => cfg.token = v.to_string(),
                        "chat_id" => cfg.chat_id = v.parse().unwrap_or(0),
                        "pair_code" => cfg.pair_code = v.to_string(),
                        "wake" => {
                            if let Some((mac, name)) = v.rsplit_once(':') {
                                let mac = mac.trim().to_uppercase();
                                if !mac.is_empty()
                                    && !name.trim().is_empty()
                                    && crate::system::parse_mac(&mac).is_some()
                                {
                                    cfg.wakes.push((mac, name.trim().to_string()));
                                } else {
                                    eprintln!("tgbot: ignoring invalid wake entry: {v}");
                                }
                            }
                        }
                        "wake_bcast" => {
                            cfg.wake_bcasts.push(v.to_string());
                        }
                        "monitor" => {
                            let (ip, name) = match v.split_once(':') {
                                Some((a, b)) => (a.to_string(), b.to_string()),
                                None => (v.to_string(), v.to_string()),
                            };
                            cfg.monitors.push((ip, name));
                        }
                        _ => {}
                    }
                }
            }
        }
        cfg
    }

    pub fn persist_chat_id(&self) {
        let s = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut out = String::new();
        let mut found = false;
        for line in s.lines() {
            if line.trim_start().starts_with("chat_id=") {
                out.push_str(&format!("chat_id={}\n", self.chat_id));
                found = true;
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        if !found {
            out.push_str(&format!("chat_id={}\n", self.chat_id));
        }
        let tmp = format!("{}.tmp", self.path);
        if std::fs::write(&tmp, &out).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_wake_entries_mac_first() {
        let path = "/tmp/tgbot-test-wake.conf";
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "wake=02:00:00:00:00:01:node1").unwrap();
        writeln!(f, "wake=02:00:00:00:00:03:node3").unwrap();
        writeln!(f, "wake=not-a-mac:node9").unwrap();
        writeln!(f, "wake=AA:BB:CC:DD:EE:FF").unwrap();
        drop(f);
        let cfg = Config::load(path);
        assert_eq!(cfg.wakes.len(), 2);
        assert_eq!(
            cfg.wakes[0],
            ("02:00:00:00:00:01".to_string(), "node1".to_string())
        );
        assert_eq!(
            cfg.wakes[1],
            ("02:00:00:00:00:03".to_string(), "node3".to_string())
        );
    }

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load("/nonexistent/tgbot.conf");
        assert!(cfg.wakes.is_empty());
        assert!(cfg.wake_bcasts.is_empty());
        assert_eq!(cfg.chat_id, 0);
    }
}
