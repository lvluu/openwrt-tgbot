# AGENTS.md

Agent operating knowledge only. Each fact lives in one place:

| Topic | Owner |
| --- | --- |
| What the bot does, bot commands, config keys, build/deploy, security model | `README.md` |
| CI gates (fmt, clippy `-D warnings`, tests, mipsel cross build, release upload) | `.github/workflows/ci.yml` |
| Toolchain download URL, linker wiring | `build.sh` |
| Router install steps | `deploy.sh` |
| procd service definition | `openwrt/tgbot.init` |

## Dev loop

```sh
cargo fmt && cargo clippy --release -- -D warnings && cargo test --release   # what CI runs
cargo test --release parses_wake                                            # one test by name filter
blaze build --release                                                       # host edit loop, ~0.1 s rebuild
./build.sh && ./deploy.sh                                                   # ship to the router
```

blaze (`./build.sh host` picks it when installed) only builds and formats here: `blaze test` and
`blaze clippy` fail on this crate (blaze cannot compile `std::thread::spawn` paths yet), and it has
no MIPS backend. Tests, clippy, the router build and CI stay on cargo.

Unit tests cover pure logic only (parsers, table/message rendering). Anything touching
`/proc`, `ubus`, `ping`, `curl` or Telegram is verified on the router: `logread | grep tgbot`.

## Architecture

```
main.rs   long-poll getUpdates (25s) ──► commands::handle_message / handle_callback
   │                                         │  shell-outs via system.rs (/proc, ubus, ping, logread)
   │                                         │  replies via telegram.rs (curl, HTML parse mode)
   │                                         └─ /wake, /watch ──spawn──► watcher.rs thread (5s poll, 3 min)
   └─ spawn at start ──► monitor.rs thread (20s presence diff over `monitor=` entries)
```

- `Config` is cloned into each thread at spawn time; there is no shared mutable state.
  Consequence: the presence monitor exits if `chat_id == 0` at startup, so after first
  pairing restart the service to get presence alerts.
- Pairing (`/start <pair_code>`) rewrites `chat_id=` in the config file in place
  (`Config::persist_chat_id`, tmp file + rename). Every message and callback from another
  chat is dropped silently.
- Inline buttons carry state in `callback_data` (`wake:<MAC>`, `reboot_yes`, `reboot_no`);
  `handle_callback` must answer with `answerCallbackQuery` or the client spinner hangs.
  The decision is the pure `decide_callback` (tested); unknown `callback_data` is ack-only,
  never a "cancelled" edit. The reboot prompt has a TTL (`REBOOT_TTL`, monotonic `Instant`).
- Replies use Telegram HTML parse mode: escape every dynamic string with `telegram::esc`,
  render tables with `tabled` inside `<pre>` (Telegram collapses plain-space alignment).
  Exception: inline-button `text` is not HTML-parsed, so leave it unescaped. Edits via
  `editMessageText` need `"parse_mode": "HTML"` too. Raw output (e.g. `/log`) goes through
  `commands::log_report`.

## Rules

- **Crate before hand-rolling** (tables `tabled`, WoL `wake-on-lan`), but only crates that
  cross-compile to mipsel musl: no async runtime, no TLS stack (why: README "Design goal").
  HTTPS stays on the router's `curl`; changing that needs a green `./build.sh` first.
- **Module per feature**; `main.rs` stays routing only.
- **Never pass the token in argv** — `ps` shows argv to every router user. `telegram::api`
  feeds the URL to `curl -K -` on stdin.
- Validate user-supplied arguments before they reach a shell-out (see `/ping`: must parse
  as `IpAddr`). `system::sh` runs `/bin/sh -c`, so never interpolate chat text into it.

## Router gotchas

- **Soft-float musl.** A hard-float binary fails with `not found` (only loader is
  `/lib/ld-musl-mipsel-sf.so.1`).
- **One poller only.** Two `tgbot` processes make Telegram silent for both. After deploy:
  `ssh $ROUTER 'ps w | grep "[t]gbot"'`, kill strays, `/etc/init.d/tgbot status`.
- Config is read at start only: restart after editing `/etc/tgbot.conf`.
- BusyBox: `sh` not `bash`, no `pkill`; kill by PID via `ps w | awk`.
