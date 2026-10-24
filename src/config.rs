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

    /// Rewrite `chat_id=` in the config file, keeping every other line and mode 0600.
    /// Never writes if the existing file cannot be read; errors are logged and returned.
    pub fn persist_chat_id(&self) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let s = std::fs::read_to_string(&self.path).map_err(|e| {
            eprintln!("tgbot: cannot read {}: {e}; not saving chat_id", self.path);
            e
        })?;
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
        // A stale tmp keeps its old mode on reopen, so start from a fresh file.
        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("tgbot: cannot remove stale {tmp}: {e}");
                return Err(e);
            }
        }
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(out.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| {
            eprintln!("tgbot: cannot save chat_id to {}: {e}", self.path);
            let _ = std::fs::remove_file(&tmp);
            e
        })
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

    fn temp_conf(name: &str, body: &str, mode: u32) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("tgbot-{}-{name}.conf", std::process::id()));
        let path = path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}.tmp"));
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn mode_of(path: &str) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    const BODY: &str = "# comment\ntoken=abc\npair_code=xyz\nchat_id=0\nmonitor=10.0.0.2:pc\nwake=02:00:00:00:00:01:node1\n";

    #[test]
    fn persist_chat_id_keeps_other_keys_and_mode_0600() {
        let path = temp_conf("persist", BODY, 0o644);
        let mut cfg = Config::load(&path);
        cfg.chat_id = 42;
        cfg.persist_chat_id().unwrap();
        assert_eq!(mode_of(&path), 0o600);
        let s = std::fs::read_to_string(&path).unwrap();
        assert_eq!(s, BODY.replace("chat_id=0", "chat_id=42"));
        let back = Config::load(&path);
        assert_eq!((back.chat_id, back.token.as_str()), (42, "abc"));
        assert_eq!(back.monitors.len(), 1);
        assert_eq!(back.wakes.len(), 1);
        assert!(!std::path::Path::new(&format!("{path}.tmp")).exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_chat_id_appends_when_missing() {
        let path = temp_conf("append", "token=abc\n", 0o600);
        let mut cfg = Config::load(&path);
        cfg.chat_id = -7;
        cfg.persist_chat_id().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "token=abc\nchat_id=-7\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_chat_id_ignores_stale_world_readable_tmp() {
        let path = temp_conf("stale", BODY, 0o600);
        let tmp = format!("{path}.tmp");
        std::fs::write(&tmp, "junk").unwrap();
        std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o644))
            .unwrap();
        let mut cfg = Config::load(&path);
        cfg.chat_id = 5;
        cfg.persist_chat_id().unwrap();
        assert_eq!(mode_of(&path), 0o600);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("token=abc"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_chat_id_aborts_when_read_fails() {
        let path = std::env::temp_dir()
            .join(format!("tgbot-{}-missing.conf", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let _ = std::fs::remove_file(&path);
        let mut cfg = Config::load(&path);
        cfg.chat_id = 9;
        assert!(cfg.persist_chat_id().is_err());
        assert!(!std::path::Path::new(&path).exists());
        assert!(!std::path::Path::new(&format!("{path}.tmp")).exists());
    }
}
