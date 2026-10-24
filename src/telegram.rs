use crate::config::Config;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const API: &str = "https://api.telegram.org/bot";
const CURL_TIMEOUT: &str = "70";

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn api(cfg: &Config, method: &str, payload: &str) -> Option<String> {
    // URL (it carries the token) goes in via `-K -` on stdin, so it never shows in `ps`.
    let mut child = Command::new("curl")
        .args([
            "-s",
            "-K",
            "-",
            "-m",
            CURL_TIMEOUT,
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            payload,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let url = format!("url = \"{}{}/{}\"\n", API, cfg.token, method);
    child.stdin.take()?.write_all(url.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        eprintln!("tgbot: {} failed: curl exited with {}", method, out.status);
        return None;
    }
    let body = String::from_utf8_lossy(&out.stdout).to_string();
    if let Some(err) = api_error(&body) {
        eprintln!("tgbot: {} rejected: {}", method, err);
    }
    Some(body)
}

/// Describes a Telegram `ok:false` reply, or None for a successful (or unparseable) body.
pub fn api_error(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    if v["ok"].as_bool() != Some(false) {
        return None;
    }
    let code = v["error_code"].as_i64().unwrap_or(0);
    let desc = v["description"].as_str().unwrap_or("no description");
    Some(format!("{} {}", code, desc))
}

pub fn send(cfg: &Config, text: &str) {
    let payload = json!({
        "chat_id": cfg.chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true
    })
    .to_string();
    let _ = api(cfg, "sendMessage", &payload);
}

pub fn get_updates(cfg: &Config, offset: i64, timeout: u64) -> Option<Vec<Value>> {
    let payload = format!(
        "{{\"offset\":{},\"timeout\":{},\"allowed_updates\":[\"message\",\"callback_query\"]}}",
        offset, timeout
    );
    let body = api(cfg, "getUpdates", &payload)?;
    let v: Value = serde_json::from_str(&body).ok()?;
    v["result"].as_array().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_specials() {
        assert_eq!(esc("a<b>&c\"d"), "a&lt;b&gt;&amp;c\"d");
        assert_eq!(esc("plain-text_1.2"), "plain-text_1.2");
    }

    #[test]
    fn api_error_reports_ok_false_only() {
        let bad =
            r#"{"ok":false,"error_code":400,"description":"Bad Request: can't parse entities"}"#;
        assert_eq!(
            api_error(bad).as_deref(),
            Some("400 Bad Request: can't parse entities")
        );
        assert_eq!(api_error(r#"{"ok":true,"result":[]}"#), None);
        assert_eq!(api_error("not json"), None);
    }
}
