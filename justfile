set shell := ["bash", "-euo", "pipefail", "-c"]

root_dir := justfile_directory()
package := "rustyget"
version := "27.0"

# Default recipe: build the release binary
default: build

# Build rustyget (`profile` = "release" or "debug", `jobs` = max parallel jobs, capped at 3 per AGENTS.md).
# Cargo builds the whole binary from Rust (rust/entry.rs is its `main`) into build/<profile>/rustyget.
# `cpu` = the Rust target-cpu tier. Empty picks the supported floor for release builds
# (x86-64-v3 on x86_64: Coffee Lake and newer; apple-m4 on Apple silicon) and the
# compiler default for debug builds. "generic" forces the compiler default. A tier other
# than the floor builds into build/<profile>-<cpu>/ so tiers can sit side by side.
build profile="release" jobs="3" cpu="":
    #!/usr/bin/env python3
    import os
    import platform
    import re
    import shutil
    import subprocess

    root = {{quote(root_dir)}}
    profile = {{quote(profile)}}
    jobs = min(int({{quote(jobs)}}), 3)
    is_debug = (profile == "debug")
    cargo_profile = "dev" if is_debug else "release"
    cargo_profile_dir = "debug" if is_debug else "release"

    system = platform.system()
    machine = platform.machine()
    cpu = {{quote(cpu)}}
    floor = {"x86_64": "x86-64-v3", "AMD64": "x86-64-v3"}.get(machine, "")
    if system == "Darwin" and machine in ("arm64", "aarch64"):
        floor = "apple-m4"
    default_cpu = "generic" if is_debug else (floor or "generic")
    cpu = cpu or default_cpu
    build_name = profile if cpu == default_cpu else f"{profile}-{cpu}"
    build_dir = os.path.join(root, "build", build_name)
    rust_target_dir = os.path.join(build_dir, "rust")
    os.makedirs(rust_target_dir, exist_ok=True)

    if system == "Darwin":
        rust_target = "aarch64-apple-darwin" if machine in ("arm64", "aarch64") else "x86_64-apple-darwin"
    else:
        out = subprocess.check_output(["rustc", "-vV"], cwd=os.path.join(root, "rust"), text=True)
        m = re.search(r"host:\s+(\S+)", out)
        rust_target = m.group(1) if m else "x86_64-unknown-linux-gnu"

    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = str(jobs)
    # The tier applies to every crate; rust/build.rs compiles the tier check
    # for the default CPU so it can still run (and explain) on older CPUs.
    if cpu != "generic":
        env["RUSTFLAGS"] = " ".join([env.get("RUSTFLAGS", ""), "-C", f"target-cpu={cpu}"]).strip()

    # On macOS use the same OpenSSL the C++ build linked (Homebrew openssl@3
    # first), so openssl-sys compiles and links against one library.
    if system == "Darwin" and not env.get("OPENSSL_DIR"):
        for prefix in ("/opt/homebrew/opt/openssl@3", "/opt/homebrew/opt/openssl", "/usr/local/opt/openssl@3", "/usr/local/opt/openssl"):
            if os.path.isdir(os.path.join(prefix, "include")):
                env["OPENSSL_DIR"] = prefix
                break

    print(f"[1/2] Building rustyget ({cargo_profile}, {rust_target}, cpu={cpu}, jobs={jobs})...")
    subprocess.run(
        [
            "cargo", "build", "--bin", "rustyget", "--locked",
            "--profile", cargo_profile,
            "--target", rust_target,
            "--manifest-path", os.path.join(root, "rust", "Cargo.toml"),
            "--target-dir", rust_target_dir,
        ],
        cwd=os.path.join(root, "rust"),
        env=env,
        check=True,
    )

    bin_path = os.path.join(build_dir, "rustyget")
    print(f"[2/2] Copying {bin_path}...")
    built = os.path.join(rust_target_dir, rust_target, cargo_profile_dir, "rustyget")
    tmp_path = bin_path + ".tmp"
    shutil.copy2(built, tmp_path)
    os.replace(tmp_path, bin_path)
    if not is_debug and shutil.which("strip"):
        subprocess.run(["strip", bin_path], check=True)
    print(f"Built {bin_path}")

# Build release binary (`build/release/rustyget`, built for the supported CPU floor)
release jobs="3":
    @just build release {{jobs}}

# Build every release CPU tier for this host's architecture: the floor into
# build/release/ plus each newer tier into build/release-<cpu>/.
# x86_64: x86-64-v3 (floor) and x86-64-v4 (AVX-512: Zen 4/5, Xeon Sapphire Rapids+).
# Apple silicon: apple-m4 only (apple-m5 adds just MTE over apple-m4 in this LLVM, so no
# measurable gain yet). Other architectures: the default only.
release-tiers jobs="3":
    #!/usr/bin/env bash
    set -euo pipefail
    case "$(uname -sm)" in
        *x86_64*) tiers="x86-64-v4" ;;
        *) tiers="" ;;
    esac
    just build release {{jobs}}
    for t in $tiers; do just build release {{jobs}} "$t"; done

# Build debug binary (`build/debug/rustyget`)
debug jobs="3":
    @just build debug {{jobs}}

# Run all tests (Rust unit tests + daemon end-to-end checks)
test jobs="3": (test-rust jobs) (test-daemon jobs)

# Run Rust library unit tests
test-rust jobs="3":
    CARGO_BUILD_JOBS={{jobs}} cargo test --manifest-path "{{root_dir}}/rust/Cargo.toml" --lib

# Build the daemon and run its end-to-end checks: `--version` and `--help` exit
# cleanly, and the direct-unpack/post-processing scenarios (formerly the C++
# PostprocessTest suite) pass against the fake news server in tests/functional/dupefallback
test-daemon jobs="3": (debug jobs)
    #!/usr/bin/env python3
    import os
    import subprocess
    import sys

    root = {{quote(root_dir)}}
    rustyget_bin = os.path.join(root, "build", "debug", "rustyget")

    print("Running ExitVersionTest...")
    subprocess.run([rustyget_bin, "--version"], check=True)
    print("Running ExitHelpTest...")
    subprocess.run([rustyget_bin, "--help"], stdout=subprocess.DEVNULL, check=True)
    scenarios = [
        "directunpacksimple", "directunpacktwoarchives", "directunpackparfailure",
        "parfailurenodirectunpack", "parfailuredirectunpackfailed",
    ]
    for scenario in scenarios:
        print(f"Running {scenario}...")
        subprocess.run([sys.executable, "-B", os.path.join(root, "tests", "functional", "dupefallback", "harness.py"),
                        "--nzbget", rustyget_bin, "--target", "local", "--scenario", scenario], check=True)
    print(f"All {2 + len(scenarios)} daemon checks passed!")

# Type-check the Rust crate
check:
    cargo check --manifest-path "{{root_dir}}/rust/Cargo.toml" --all-targets

# Run Clippy on the Rust crate
clippy:
    cargo clippy --manifest-path "{{root_dir}}/rust/Cargo.toml" --lib

# Score how idiomatic the Rust is (docs/nativeness/README.md); `clippy`="no" skips the slow clippy pass
nativeness clippy="yes":
    python3 "{{root_dir}}/scripts/nativeness/score.py" {{ if clippy == "yes" { "--run-clippy --compare " + quote(root_dir / "docs/nativeness/baseline.json") } else { "" } }}

# Format the Rust crate
fmt:
    cargo fmt --manifest-path "{{root_dir}}/rust/Cargo.toml"

# Update the local package directory (defaults to ~/Documents/rustyget)
package dest=(env_var("HOME") / "Documents/rustyget") jobs="3": (release jobs)
    mkdir -p "{{dest}}"
    cp "{{root_dir}}/build/release/rustyget" "{{dest}}/rustyget"
    cp "{{root_dir}}/rustyget.conf" "{{dest}}/rustyget.conf.template"
    rsync -a --delete "{{root_dir}}/webui/" "{{dest}}/webui/"
    cp "{{root_dir}}/README.md" "{{dest}}/README.md"

# Install rustyget binary, webui, docs, and config template into `prefix`
install prefix="/usr/local" jobs="3": (release jobs)
    mkdir -p "{{prefix}}/bin" "{{prefix}}/share/{{package}}/doc" "{{prefix}}/share/{{package}}/webui"
    install -m 0755 "{{root_dir}}/build/release/rustyget" "{{prefix}}/bin/{{package}}"
    cp -R "{{root_dir}}/webui/." "{{prefix}}/share/{{package}}/webui/"
    install -m 0644 "{{root_dir}}/ChangeLog.md" "{{root_dir}}/COPYING" "{{prefix}}/share/{{package}}/doc/"
    sed -e "s|^WebDir=.*|WebDir={{prefix}}/share/{{package}}/webui|" \
        -e "s|^ConfigTemplate=.*|ConfigTemplate={{prefix}}/share/{{package}}/rustyget.conf|" \
        "{{root_dir}}/rustyget.conf" > "{{prefix}}/share/{{package}}/rustyget.conf"

# Uninstall rustyget binary, webui, and docs from `prefix`
uninstall prefix="/usr/local":
    rm -rf "{{prefix}}/share/{{package}}"
    rm -f "{{prefix}}/bin/{{package}}"

# Install rustyget.conf into `prefix/etc`
install-conf prefix="/usr/local":
    mkdir -p "{{prefix}}/etc"
    sed -e "s|^WebDir=.*|WebDir={{prefix}}/share/{{package}}/webui|" \
        -e "s|^ConfigTemplate=.*|ConfigTemplate={{prefix}}/share/{{package}}/rustyget.conf|" \
        "{{root_dir}}/rustyget.conf" > "{{prefix}}/etc/rustyget.conf"

# Remove rustyget.conf from `prefix/etc`
uninstall-conf prefix="/usr/local":
    rm -f "{{prefix}}/etc/rustyget.conf"

# Clean build artifacts
clean:
    rm -rf "{{root_dir}}/build" "{{root_dir}}/rust/target"
