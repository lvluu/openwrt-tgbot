# openwrt-tgbot

[![CI](https://github.com/lvluu/openwrt-tgbot/actions/workflows/ci.yml/badge.svg)](https://github.com/lvluu/openwrt-tgbot/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Tiny Telegram bot to control and monitor an OpenWrt router.

Runs as a procd service on the router. Built and tested on ramips/mt7621 (mipsel, soft-float musl,
e.g. OpenWrt/X-WRT); other targets need their own cross toolchain.

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

Also pushes a message when a `monitor=` device joins or leaves the LAN.

"Online" means the kernel confirmed the neighbour (`ip neigh` state REACHABLE) or it answers a ping;
a stale ARP entry alone does not count. A monitored device is reported offline after two missed
checks in a row, usually within about a minute and at most ~90 s with default kernel neighbour timers (a device stays REACHABLE for up to ~45 s after power-off). Hosts that drop ICMP while idle can read as offline.

## Build

```sh
./build.sh          # cross-compile for the router (nightly + build-std)
./build.sh host     # native build
```

Requirements: Rust nightly with `rust-src` (`rustup toolchain install nightly --component rust-src`)
and a mipsel soft-float musl cross toolchain:

`./build.sh` prints the download command when the toolchain is missing. Any
`mipsel-*muslsf-gcc` toolchain works; point `TOOLCHAIN_DIR=...` at it. It must be the
soft-float (`sf`) variant.

## Prebuilt binary

CI cross-compiles `tgbot` for mipsel soft-float musl on every push: grab it from the
workflow run artifacts, or from the release page for `v*` tags.

## Deploy

```sh
./deploy.sh                          # default: root@192.168.1.1
ROUTER=root@my-router ./deploy.sh    # any ssh target (LAN IP, Tailscale name, ...)
```

`deploy.sh` copies the binary and init script, then restarts the service. The paired
`chat_id` persists in the router's config across redeploys.

## Router files

| Path | Purpose |
| --- | --- |
| `/usr/bin/tgbot` | binary |
| `/etc/tgbot.conf` | config, keys below |
| `/etc/init.d/tgbot` | procd service (respawns on crash) |

Config format (`openwrt/tgbot.conf.example`), one `key=value` per line:

| Key | Meaning |
| --- | --- |
| `token` | bot token from [@BotFather](https://t.me/BotFather) |
| `chat_id` | paired chat; leave `0`, `/start <pair_code>` fills it in |
| `pair_code` | secret you send with `/start` to pair |
| `monitor=<ip>:<name>` | push a message when this device joins/leaves (repeatable) |
| `wake=<mac>:<name>` | machine offered by `/wake` (repeatable) |
| `wake_bcast=<addr>` | extra WoL broadcast address, for machines on another subnet (repeatable) |

## Security notes

- The token lives only in `/etc/tgbot.conf` on the router (`chmod 600`), never in this repo.
- The token is handed to `curl` on stdin, so it never shows up in `ps` output.
- If a token leaks, revoke it via [@BotFather](https://t.me/BotFather) (`/revoke`) and update the config.
- Only the paired `chat_id` is served; everyone else is ignored.

## License

[MIT](LICENSE)

## Contributing

Dev loop, architecture and router gotchas: [AGENTS.md](AGENTS.md).
