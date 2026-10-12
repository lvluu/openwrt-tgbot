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
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
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
}
