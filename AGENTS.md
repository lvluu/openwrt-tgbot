# AGENTS.md

User docs: **README.md**. This file is only operating knowledge the code can't tell you.

## Deploy

```sh
cargo test --release                                   # always
./build.sh                                             # mipsel + nightly + build-std
./deploy.sh                                            # ROUTER defaults to root@192.168.1.1
ROUTER=root@my-router ./deploy.sh                      # any ssh target
```

`./deploy.sh` = scp binary + init script, restart service. That's the whole release flow.

## Principles

- **Never reinvent the wheel.** Reach for a crate first: tables → `tabled` (+ `<pre>`,
  Telegram collapses plain spaces), WoL → `wake-on-lan`. Hand-rolled padding/parsers only
  when no library fits, and keep the reason in the PR.
- **Keep the module split** (`config` / `telegram` / `system` / `commands` / `monitor` /
  `main`). New features get a module, not another 200 lines in `main.rs`.
- **Minimal deps that cross-compile.** No async runtimes, no reqwest/ureq/teloxide:
  rustls/ring have no MIPS target. HTTPS rides the router's `curl` — don't change this
  without checking the cross build.
- Tests before deploy; `cargo test --release` covers the pure logic.

## Gotchas

- Router is **soft-float musl**: build with a `mipsel-*muslsf` toolchain (`build.sh` prints where to get one) or the binary dies
  with `not found` (loader is `/lib/ld-musl-mipsel-sf.so.1`).
- **One bot process only** — two pollers make Telegram silent for both. After deploy:
  `ssh $ROUTER 'ps w | grep "[t]gbot"'`, kill strays, `/etc/init.d/tgbot status`.
- Config is read at start only → restart after editing `/etc/tgbot.conf`.
- Verify bot behaviour on the router (`logread | grep tgbot`), not only via unit tests.
- Router runs BusyBox: `sh` not `bash`, no `pkill` — kill by PID via `ps w | awk`.
- Token never goes in git. `/etc/tgbot.conf` on the router, `chmod 600`.

## Map

```
src/main.rs      poll loop, update routing
src/config.rs    /etc/tgbot.conf parsing
src/telegram.rs  Bot API via curl, HTML escaping
src/system.rs    /proc + ubus readers, ping, WoL
src/commands.rs  handlers, tables, callbacks, unit tests
src/monitor.rs   LAN presence watcher (20s)
src/watcher.rs   post-wake "is it up" watcher (5s poll, 3 min deadline)
openwrt/         init script, config example
```
