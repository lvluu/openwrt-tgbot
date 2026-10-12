mod commands;
mod config;
mod monitor;
mod system;
mod telegram;
mod watcher;

use config::Config;

const POLL_TIMEOUT: u64 = 25;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg_path = args
        .iter()
        .position(|a| a == "-c")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "/etc/tgbot.conf".to_string());

    let mut cfg = Config::load(&cfg_path);
    if cfg.token.is_empty() {
        eprintln!("tgbot: no token in {}", cfg_path);
        std::process::exit(1);
    }

    let mon = cfg.clone();
    std::thread::spawn(move || monitor::run(mon));

    let mut offset: i64 = 0;
    loop {
        match telegram::get_updates(&cfg, offset, POLL_TIMEOUT) {
            Some(updates) => {
                for upd in &updates {
                    if let Some(id) = upd["update_id"].as_i64() {
                        offset = id + 1;
                    }
                    if let Some(msg) = upd.get("message") {
                        let chat = msg["chat"]["id"].as_i64().unwrap_or(0);
                        let text = msg["text"].as_str().unwrap_or("").trim().to_string();
                        if chat != 0 && !text.is_empty() {
                            commands::handle_message(&mut cfg, chat, &text);
                        }
                    } else if let Some(cb) = upd.get("callback_query") {
                        commands::handle_callback(&cfg, cb);
                    }
                }
            }
            None => std::thread::sleep(std::time::Duration::from_secs(5)),
        }
    }
}
