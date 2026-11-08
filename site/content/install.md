+++
title = "Install and build"
description = "Build RustyGet from source on Linux or macOS with Just and the pinned Rust toolchain, then run it as a service."
eyebrow = "Get started"
lede = "RustyGet runs on Linux and macOS, and builds from source with Just, a C++20 compiler, and Rust. Release packages come back once the build workflows are rebuilt for the rewrite."
+++

> [!IMPORTANT] No release packages yet
> The NZBGet installers, Docker images, and NAS packages install stock NZBGet, not RustyGet. Until RustyGet publishes its own packages, build it from source.

## Requirements

| Tool or library | Version | Notes |
|---|---|---|
| `just` | any current | Command runner used for building, testing, and packaging (`justfile`). |
| C++ compiler | GCC 11+ or Clang 14+ | C++20 support is required while the remaining glue headers are ported to Rust. |
| Rust | 1.99.0 | Pinned in `rust/rust-toolchain.toml`. With rustup, the right toolchain installs itself on the first build. |
| OpenSSL | any current | For encrypted news-server and HTTPS connections |

### Install the libraries

Debian 13 and Ubuntu 24.04:

```bash
sudo apt install just build-essential pkg-config libssl-dev
```

macOS with Homebrew:

```bash
xcode-select --install
brew install just openssl
```

### Install Rust

Install rustup from [rustup.rs](https://rustup.rs), then check that `cargo` is on your path:

```bash
cargo --version
```

You don't need to pick a version. The repository pins Rust 1.99.0, and rustup installs it the first time the build runs `cargo` inside `rust/`.

## Build

```bash
git clone https://github.com/Appz4Fun/RustyGet.git rustyget
cd rustyget
just build
```

The release binary is `build/release/rustyget`.

To install it system-wide into `/usr/local/bin` along with the web interface and configuration template in `/usr/local/share/rustyget`:

```bash
sudo install -d /usr/local/bin /usr/local/share/rustyget /usr/local/etc
sudo install -m 755 build/release/rustyget /usr/local/bin/rustyget
sudo cp -R webui /usr/local/share/rustyget/
sudo install -m 644 rustyget.conf /usr/local/share/rustyget/rustyget.conf
[ -e /usr/local/etc/rustyget.conf ] || sudo install -m 600 rustyget.conf /usr/local/etc/rustyget.conf
```

> [!WARNING] Upgrades: keep your configuration file
> Overwriting `/usr/local/etc/rustyget.conf` with the default template loses your servers and settings. Copy `rustyget.conf` into `/usr/local/etc/rustyget.conf` only on a first install.

To package a self-contained installation into a local folder (`~/Documents/rustyget` by default), run:

```bash
just package
```

To run RustyGet as your own user with a private configuration file:

```bash
[ -e ~/.rustyget ] || install -m 600 rustyget.conf ~/.rustyget
```

The test keeps an existing `~/.rustyget` when you run the steps again for an upgrade.

### Build recipes

| Recipe | Effect |
|---|---|
| `just build` | Builds the optimized release binary at `build/release/rustyget`. |
| `just build-debug` | Builds the debug binary with test suites at `build/debug/rustyget`. |
| `just test` | Runs the Rust unit tests (`just test-rust`) and daemon test suites (`just test-daemon`). |
| `just package [dest]` | Builds and stages a self-contained bundle with `rustyget`, `rustyget.conf`, and `webui/`. |

## First start

Copy the default configuration, then start RustyGet in the foreground from the source tree to check it:

```bash
[ -e ~/rustyget.conf ] || install -m 600 rustyget.conf ~/rustyget.conf
build/release/rustyget -c ~/rustyget.conf -o WebDir="$PWD/webui" -o ConfigTemplate="$PWD/rustyget.conf" -s
```

Downloads go under `~/downloads` until you change `MainDir`.

Then open the web interface at `http://<host>:6789`. The default login is `rustyget` with the password `tegbzn6789`. Change it in **SECURITY** before you expose the port. [Quick start](quick-start.md) walks through the first settings.

If the terminal interface can't open your terminal, add `-o outputmode=log`.

## Run as a service

On Linux with systemd, run RustyGet as its own user. Create the user with a home directory, which becomes the base of the default `MainDir`, `~/downloads`, and give it the installed configuration file, which the web interface writes to:

```bash
sudo useradd --system --create-home --home-dir /var/lib/rustyget rustyget
sudo chown rustyget: /usr/local/etc/rustyget.conf
sudo chmod 600 /usr/local/etc/rustyget.conf
```

The file holds your news-server passwords and API keys once you save settings, so only the service user may read it.

Then add a unit such as `/etc/systemd/system/rustyget.service`:

```ini
[Unit]
Description=RustyGet Usenet downloader
After=network-online.target
Wants=network-online.target

[Service]
User=rustyget
Group=rustyget
Type=forking
ExecStart=/usr/local/bin/rustyget -D -c /usr/local/etc/rustyget.conf
ExecStop=/usr/local/bin/rustyget -Q -c /usr/local/etc/rustyget.conf
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Start it with `sudo systemctl enable --now rustyget`. These paths assume `/usr/local`; adjust them if you installed into another prefix.

On other systems, start RustyGet with `rustyget -D` from your init system and stop it with `rustyget -Q`.

## Move from NZBGet

RustyGet reads NZBGet's configuration file, queue, and history, so you can point it at an existing setup.

1. Stop NZBGet.
2. Back up the queue directory (`QueueDir`) and `nzbget.conf`.
3. Start RustyGet with the same configuration file.

RustyGet writes the queue in NZBGet's format and keeps its own extra data in a trailer that NZBGet ignores, so NZBGet can still read the queue afterward. Some RustyGet-only state is lost if you go back.

> [!WARNING] Back up before you switch back
> If you return to NZBGet, back up the queue directory first. NZBGet keeps the queue and history but drops RustyGet's extra data, such as stream-repair progress and recovery counters, the first time it saves.

The new settings all default to NZBGet's behavior. Turn on the duplicate features one at a time, as described in [Duplicate fleets](duplicate-fleets.md).
