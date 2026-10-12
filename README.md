# openwrt-tgbot

[![CI](https://github.com/lvluu/openwrt-tgbot/actions/workflows/ci.yml/badge.svg)](https://github.com/lvluu/openwrt-tgbot/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Tiny Telegram bot to control and monitor an OpenWrt router.

Runs as a procd service on the router. Built and tested on ramips/mt7621 (mipsel, soft-float musl,
e.g. OpenWrt/X-WRT), but any target Rust can cross-compile to works. Device presence alerts included.

Design goal: minimal resource use. One static `mipsel-unknown-linux-musl` binary (~570 KB),
single-threaded long-poll loop, HTTPS via the router's own `curl` (no TLS library linked in,
which also sidesteps rustls/ring having no MIPS support). Typical RSS ~4 MB.

## Commands

| Command | What it does |
| --- | --- |
| `/start <pair_code>` | pairs your chat; bot ignores everyone else until then |
| `/status` | uptime, load, memory, CPU temp, WAN IP, device count |
| `/clients` | DHCP leases table with 🟢/⚪ online state |
| `/wake` | clickable buttons, one per machine → sends Wake-on-LAN magic packet, then watches for it to come online |
| `/watch <machine>` | watch a machine (by name or MAC) come online for 3 minutes |
| `/ping <ip>` | LAN reachability check (IP address only) |
| `/tailscale` | tailnet nodes as an aligned table |
| `/log [n]` | last n log lines |
| `/restartwifi` | restart networking |
| `/reboot` | reboot with an inline confirm/cancel button |

Tables are rendered with the [`tabled`](https://crates.io/crates/tabled) crate inside
`<pre>` blocks (box-drawing characters align reliably; plain spaces get collapsed by
Telegram clients). Wake-on-LAN uses [`wake-on-lan`](https://crates.io/crates/wake-on-lan)
and can blast to extra broadcast domains via config (needed when the target machine sits
on a different subnet, e.g. behind an ISP gateway).

Also pushes messages when a monitored device joins/leaves the LAN.

## Build

```sh
./build.sh          # cross-compile for the router (nightly + build-std)
./build.sh host     # native build, useful for `cargo test`
cargo test --release
```

Requirements: Rust nightly with `rust-src` (`rustup toolchain install nightly --component rust-src`)
and a mipsel soft-float musl cross toolchain:

```sh
mkdir -p toolchain && curl -fsSL https://github.com/cross-tools/musl-cross/releases/download/20261001/mipsel-unknown-linux-muslsf.tar.xz | tar xJ -C toolchain
```

Any `mipsel-*muslsf-gcc` toolchain works (musl.cc too); point `TOOLCHAIN_DIR=...` at it (the `sf` variant matters — many mt7621 builds ship soft-float musl, hard-float binaries get
"not found" because `/lib/ld-musl-mipsel-sf.so.1` is the only loader).

## Prebuilt binary

CI cross-compiles `tgbot` for mipsel soft-float musl on every push: grab it from the
workflow run artifacts, or from the release page for `v*` tags.

## Deploy

```sh
./deploy.sh                          # default: root@192.168.1.1
ROUTER=root@my-router ./deploy.sh    # any ssh target (LAN IP, Tailscale name, ...)
```

`deploy.sh` scp's the binary and init script, then restarts the service. Redeploys are
always: rebuild → scp → restart. The paired `chat_id` persists in the router's config.

## Router files

| Path | Purpose |
| --- | --- |
| `/usr/bin/tgbot` | binary |
| `/etc/tgbot.conf` | config (token, chat_id, pair_code, monitor/wake lists) |
| `/etc/init.d/tgbot` | procd service (`START=95`, respawn on crash) |

Config format (`openwrt/tgbot.conf.example`), one `key=value` per line:

| Key | Meaning |
| --- | --- |
| `token` | bot token from [@BotFather](https://t.me/BotFather) |
| `chat_id` | paired chat; leave `0`, `/start <pair_code>` fills it in |
| `pair_code` | secret you send with `/start` to pair |
| `monitor=<ip>:<name>` | push a message when this device joins/leaves (repeatable) |
| `wake=<mac>:<name>` | machine offered by `/wake` (repeatable) |
| `wake_bcast=<addr>` | extra broadcast address for WoL packets (repeatable) |

## Security notes

- The token lives only in `/etc/tgbot.conf` on the router (`chmod 600`), never in this repo.
- The token is handed to `curl` on stdin, so it never shows up in `ps` output.
- If a token leaks, revoke it via [@BotFather](https://t.me/BotFather) (`/revoke`) and update the config.
- Only the paired `chat_id` is served; everyone else is ignored.

## License

[MIT](LICENSE)

## For AI agents

See [AGENTS.md](AGENTS.md) — build/deploy workflow, remote (Tailscale) access, and the
gotchas that bite when touching this code.
