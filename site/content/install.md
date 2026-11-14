+++
title = "Install and build"
description = "Build RustyGet from source on Linux or macOS with Just and the pinned Rust toolchain, then run it as a service."
eyebrow = "Get started"
lede = "RustyGet runs on Linux and macOS, and builds from source with Just, a C++20 compiler, and Rust. Release packages come back once the build workflows are rebuilt for the rewrite."
+++

> [!WARNING] Linux builds on the main branch
> The Rust library on the current main branch doesn't compile on Linux, because it uses thread-cancellation constants that the `libc` crate defines only for Apple targets. A fix is on the way. Until it lands, main builds on macOS only.

> [!IMPORTANT] No release packages yet
> The NZBGet installers, Docker images, and NAS packages install stock NZBGet, not RustyGet. Until RustyGet publishes its own packages, build it from source.

## Requirements

| Tool or library | Version | Notes |
|---|---|---|
| `just` | any current | Command runner used for building, testing, and packaging (`justfile`). |
| C++ compiler | GCC 11+ or Clang 14+ | C++20 support is required for the entry point and the glue header that are still in C++. |
| Rust | 1.99.0 | Pinned in `rust/rust-toolchain.toml`. With rustup, the right toolchain installs itself on the first build. |
| OpenSSL | any current | For encrypted news-server and HTTPS connections |

RustyGet needs no `par2`, `unrar`, or `7z` program: par2 repair and archive extraction are built in.

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

The configuration template is `nzbget.conf` in the repository; it keeps NZBGet's filename for now and installs as `rustyget.conf`.

To install RustyGet system-wide into `/usr/local/bin`, with the web interface and the configuration template in `/usr/local/share/rustyget`, run these commands from the repository. The `sed` command points the template's `WebDir` and `ConfigTemplate` at the installed files, as `just install` does:

```bash
sudo install -d /usr/local/bin /usr/local/share/rustyget /usr/local/etc
sudo install -m 755 build/release/rustyget /usr/local/bin/rustyget
sudo cp -R webui /usr/local/share/rustyget/
sed -e 's|^WebDir=.*|WebDir=/usr/local/share/rustyget/webui|' \
    -e 's|^ConfigTemplate=.*|ConfigTemplate=/usr/local/share/rustyget/rustyget.conf|' \
    nzbget.conf | sudo tee /usr/local/share/rustyget/rustyget.conf > /dev/null
[ -e /usr/local/etc/rustyget.conf ] || sudo install -m 600 /usr/local/share/rustyget/rustyget.conf /usr/local/etc/rustyget.conf
```

> [!WARNING] Upgrades: keep your configuration file
> Overwriting `/usr/local/etc/rustyget.conf` with the default template loses your servers and settings. The last command copies the template only on a first install. `just install-conf` overwrites the file every time, so don't use it for an upgrade.

To copy the binary, the template as `rustyget.conf.template`, the web interface, and the README into a local folder (`~/Documents/rustyget` by default), run:

```bash
just package
```

To run RustyGet as your own user with a private configuration file:

```bash
[ -e ~/.rustyget ] || install -m 600 nzbget.conf ~/.rustyget
```

The test keeps an existing `~/.rustyget` when you run the steps again for an upgrade.

### Build recipes

| Recipe | Effect |
|---|---|
| `just build` | Builds the optimized release binary at `build/release/rustyget`. `just release` does the same. |
| `just debug` | Builds the debug binary at `build/debug/rustyget`. |
| `just test` | Runs the Rust unit tests (`just test-rust`) and the daemon tests (`just test-daemon`). |
| `just check`, `just clippy`, `just fmt` | Type-checks, lints, or formats the Rust crate. |
| `just package [dest]` | Builds and copies `rustyget`, `rustyget.conf.template`, `webui/`, and the README into a folder. |
| `just install [prefix]`, `just uninstall [prefix]` | Builds and installs, or removes, the binary, web interface, documents, and template under a prefix, `/usr/local` by default. |
| `just install-conf [prefix]` | Writes `rustyget.conf` into `prefix/etc`, replacing any file there. |
| `just clean` | Deletes the build output. |

## First start

Copy the default configuration, then start RustyGet in the foreground from the source tree to check it:

```bash
[ -e ~/rustyget.conf ] || install -m 600 nzbget.conf ~/rustyget.conf
build/release/rustyget -c ~/rustyget.conf -o WebDir="$PWD/webui" -o ConfigTemplate="$PWD/nzbget.conf" -s
```

Downloads go under `~/downloads` until you change `MainDir`. The template ships with `Server1.Active=no`, so nothing downloads until you add and turn on a news server.

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

Options that your NZBGet configuration file doesn't list take RustyGet's built-in values. The duplicate-fleet settings are off in those, so your setup behaves as before until you turn them on, one at a time, as described in [Duplicate fleets](duplicate-fleets.md). A few other built-in values differ from NZBGet's; [Differences from NZBGet](differences.md#defaults) lists them.
