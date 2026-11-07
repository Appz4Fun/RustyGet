set shell := ["bash", "-euo", "pipefail", "-c"]

root_dir := justfile_directory()
package := "rustyget"
version := "27.0"

# Default recipe: build the release binary
default: build

# Build rustyget (`profile` = "release" or "debug", `jobs` = max parallel jobs, capped at 3 per AGENTS.md)
build profile="release" jobs="3":
    #!/usr/bin/env python3
    import concurrent.futures
    import glob
    import hashlib
    import os
    import platform
    import re
    import shutil
    import subprocess
    import sys

    root = {{quote(root_dir)}}
    profile = {{quote(profile)}}
    jobs = min(int({{quote(jobs)}}), 3)
    is_debug = (profile == "debug")
    cargo_profile = "dev" if is_debug else "release"
    cargo_profile_dir = "debug" if is_debug else "release"

    build_dir = os.path.join(root, "build", profile)
    obj_dir = os.path.join(build_dir, "obj")
    rust_target_dir = os.path.join(build_dir, "rust")
    os.makedirs(obj_dir, exist_ok=True)
    os.makedirs(rust_target_dir, exist_ok=True)

    system = platform.system()
    machine = platform.machine()
    if system == "Darwin":
        rust_target = "aarch64-apple-darwin" if machine in ("arm64", "aarch64") else "x86_64-apple-darwin"
    else:
        out = subprocess.check_output(["rustc", "-vV"], cwd=os.path.join(root, "rust"), text=True)
        m = re.search(r"host:\s+(\S+)", out)
        rust_target = m.group(1) if m else "x86_64-unknown-linux-gnu"

    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = str(jobs)

    print(f"[1/4] Building Rust staticlib ({cargo_profile}, {rust_target}, jobs={jobs})...")
    subprocess.run(
        [
            "cargo", "rustc", "--lib", "--locked",
            "--profile", cargo_profile,
            "--target", rust_target,
            "--manifest-path", os.path.join(root, "rust", "Cargo.toml"),
            "--target-dir", rust_target_dir,
        ],
        cwd=os.path.join(root, "rust"),
        env=env,
        check=True,
    )
    rust_lib = os.path.join(rust_target_dir, rust_target, cargo_profile_dir, "libnzbget_rs.a")

    entry_obj = os.path.join(rust_target_dir, "nzbget_rs_entry.o")
    entry_src = os.path.join(root, "rust", "entry.rs")
    if not os.path.exists(entry_obj) or os.path.getmtime(entry_src) > os.path.getmtime(entry_obj):
        print("[2/4] Building Rust entry point...")
        subprocess.run(
            [
                "rustc", "--crate-name", "nzbget_rs_entry", "--crate-type", "lib",
                "--emit=obj", "-C", "panic=abort", "-C", "opt-level=2",
                "--target", rust_target, entry_src, "-o", entry_obj,
            ],
            cwd=os.path.join(root, "rust"),
            env=env,
            check=True,
        )
    else:
        print("[2/4] Rust entry point up to date.")

    cxx = env.get("CXX") or ("clang++" if shutil.which("clang++") else "g++")
    ccache = shutil.which("ccache")

    # Resolve OpenSSL include/lib flags
    openssl_cflags = []
    openssl_ldflags = ["-lssl", "-lcrypto"]
    if system == "Darwin":
        for prefix in ("/opt/homebrew/opt/openssl@3", "/opt/homebrew/opt/openssl", "/usr/local/opt/openssl@3", "/usr/local/opt/openssl"):
            if os.path.isdir(os.path.join(prefix, "include")):
                openssl_cflags = [f"-I{os.path.join(prefix, 'include')}"]
                openssl_ldflags = [f"-L{os.path.join(prefix, 'lib')}", "-lssl", "-lcrypto"]
                break
    elif shutil.which("pkg-config"):
        try:
            cflags_out = subprocess.check_output(["pkg-config", "--cflags", "openssl"], text=True).strip()
            libs_out = subprocess.check_output(["pkg-config", "--libs", "openssl"], text=True).strip()
            if cflags_out:
                openssl_cflags = cflags_out.split()
            if libs_out:
                openssl_ldflags = libs_out.split()
        except Exception:
            pass

    daemon_Comparison_dirs = [
        "connect", "extension", "feed", "frontend", "main",
        "nntp", "nserv", "postprocess", "queue", "dupesearch",
        "remote", "system", "util", "systemhealth",
    ]
    include_flags = [f"-I{root}", f"-I{os.path.join(root, 'rust', 'include')}"]
    for d in daemon_Comparison_dirs:
        include_flags.append(f"-I{os.path.join(root, 'daemon', d)}")
    include_flags.extend(openssl_cflags)

    common_flags = [
        "-std=c++20",
        "-fvisibility=hidden",
        "-fvisibility-inlines-hidden",
        "-D_FILE_OFFSET_BITS=64",
        "-D_LARGEFILE_SOURCE",
        "-D_LARGE_FILES",
        "-DNZBGET_USE_RUST=1",
    ]
    if is_debug:
        common_flags.extend(["-O0", "-g", "-DDEBUG"])
    else:
        common_flags.extend([
            "-O2", "-DNDEBUG", "-DOPENSSL_SUPPRESS_DEPRECATED",
            "-fno-rtti", "-ffunction-sections", "-fdata-sections",
        ])

    def needs_rebuild(src, obj, depfile):
        if not os.path.exists(obj) or not os.path.exists(depfile):
            return True
        obj_mtime = os.path.getmtime(obj)
        if os.path.getmtime(src) > obj_mtime:
            return True
        try:
            with open(depfile, "r", encoding="utf-8") as f:
                content = f.read().replace("\\\n", " ")
            parts = content.split(":", 1)
            if len(parts) == 2:
                for dep in parts[1].split():
                    if not os.path.exists(dep) or os.path.getmtime(dep) > obj_mtime:
                        return True
        except Exception:
            return True
        return False

    cpp_sources = sorted(glob.glob(os.path.join(root, "daemon", "*", "*.cpp")))
    lib_sources = [s for s in cpp_sources if not s.endswith(os.path.join("daemon", "main", "nzbget.cpp"))]
    main_source = os.path.join(root, "daemon", "main", "nzbget.cpp")

    def obj_path_for(src):
        rel = os.path.relpath(src, root).replace(os.sep, "__")
        return os.path.join(obj_dir, rel + ".o")

    def compile_one(src):
        obj = obj_path_for(src)
        dep = obj + ".d"
        if not needs_rebuild(src, obj, dep):
            return obj, False
        cmd = ([ccache] if ccache else []) + [
            cxx, *common_flags, *include_flags,
            "-MMD", "-MF", dep, "-c", src, "-o", obj,
        ]
        subprocess.run(cmd, check=True)
        return obj, True

    print(f"[3/4] Compiling C++ daemon sources ({len(cpp_sources)} files, jobs={jobs})...")
    all_objs = []
    rebuilt_count = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = [pool.submit(compile_one, s) for s in lib_sources + [main_source]]
        for fut in concurrent.futures.as_completed(futures):
            obj, rebuilt = fut.result()
            if rebuilt:
                rebuilt_count += 1

    lib_objs = [obj_path_for(s) for s in lib_sources]
    main_obj = obj_path_for(main_source)

    archive_path = os.path.join(build_dir, "librustyget.a")
    if os.path.exists(archive_path):
        os.remove(archive_path)
    subprocess.run(["ar", "rcs", archive_path, *lib_objs], check=True)

    bin_path = os.path.join(build_dir, "rustyget")
    link_flags = ["-pthread"]
    if system == "Darwin":
        link_flags.extend(["-Wl,-headerpad_max_install_names", "-Wl,-dead_strip", "-liconv", "-lresolv"])
    else:
        link_flags.extend(["-Wl,--gc-sections", "-Wl,-z,relro,-z,now", "-Wl,-z,noexecstack", "-ldl", "-lm"])

    print(f"[4/4] Linking {bin_path}...")
    subprocess.run(
        [cxx, main_obj, entry_obj, archive_path, rust_lib, *openssl_ldflags, *link_flags, "-o", bin_path],
        check=True,
    )
    if not is_debug and shutil.which("strip"):
        subprocess.run(["strip", bin_path], check=True)
    print(f"Built {bin_path}")

# Build release binary (`build/release/rustyget`)
release jobs="3":
    @just build release {{jobs}}

# Build debug binary (`build/debug/rustyget`)
debug jobs="3":
    @just build debug {{jobs}}

# Run all tests (Rust unit tests + C++ daemon/integration tests)
test jobs="3": (test-rust jobs) (test-daemon jobs)

# Run Rust library unit tests
test-rust jobs="3":
    CARGO_BUILD_JOBS={{jobs}} cargo test --manifest-path "{{root_dir}}/rust/Cargo.toml" --lib

# Build and run the C++ daemon & integration tests (`PostprocessTest`, `ExitVersionTest`, `ExitHelpTest`, `ConnectTest`)
test-daemon jobs="3": (debug jobs)
    #!/usr/bin/env python3
    import concurrent.futures
    import os
    import platform
    import shutil
    import subprocess

    root = {{quote(root_dir)}}
    jobs = min(int({{quote(jobs)}}), 3)
    build_dir = os.path.join(root, "build", "debug")
    test_dir = os.path.join(build_dir, "tests")
    test_obj_dir = os.path.join(test_dir, "obj")
    os.makedirs(test_obj_dir, exist_ok=True)

    system = platform.system()
    machine = platform.machine()
    rust_target = (
        ("aarch64-apple-darwin" if machine in ("arm64", "aarch64") else "x86_64-apple-darwin")
        if system == "Darwin"
        else "x86_64-unknown-linux-gnu"
    )
    rust_lib = os.path.join(build_dir, "rust", rust_target, "debug", "libnzbget_rs.a")
    archive_path = os.path.join(build_dir, "librustyget.a")
    rustyget_bin = os.path.join(build_dir, "rustyget")

    cxx = os.environ.get("CXX") or ("clang++" if shutil.which("clang++") else "g++")
    ccache = shutil.which("ccache")

    openssl_cflags = []
    openssl_ldflags = ["-lssl", "-lcrypto"]
    if system == "Darwin":
        for prefix in ("/opt/homebrew/opt/openssl@3", "/opt/homebrew/opt/openssl", "/usr/local/opt/openssl@3", "/usr/local/opt/openssl"):
            if os.path.isdir(os.path.join(prefix, "include")):
                openssl_cflags = [f"-I{os.path.join(prefix, 'include')}"]
                openssl_ldflags = [f"-L{os.path.join(prefix, 'lib')}", "-lssl", "-lcrypto"]
                break

    daemon_dirs = [
        "connect", "extension", "feed", "frontend", "main",
        "nntp", "nserv", "postprocess", "queue", "dupesearch",
        "remote", "system", "util", "systemhealth",
    ]
    include_flags = [
        f"-I{root}",
        f"-I{os.path.join(root, 'tests')}",
        f"-I{os.path.join(root, 'rust', 'include')}",
    ]
    for d in daemon_dirs:
        include_flags.append(f"-I{os.path.join(root, 'daemon', d)}")
    include_flags.extend(openssl_cflags)

    flags = [
        "-std=c++20", "-O0", "-g", "-DDEBUG",
        "-fvisibility=hidden", "-fvisibility-inlines-hidden",
        "-D_FILE_OFFSET_BITS=64", "-D_LARGEFILE_SOURCE", "-D_LARGE_FILES",
        "-DNZBGET_USE_RUST=1",
    ]

    test_sources = [
        os.path.join(root, "tests", "main.cpp"),
        os.path.join(root, "tests", "postprocess", "DirectUnpack.cpp"),
        os.path.join(root, "tests", "postprocess", "ParChecker.cpp"),
        os.path.join(root, "tests", "connect", "TlsCipher.cpp"),
    ]

    def compile_test(src):
        rel = os.path.relpath(src, root).replace(os.sep, "__")
        obj = os.path.join(test_obj_dir, rel + ".o")
        dep = obj + ".d"
        if not os.path.exists(obj) or os.path.getmtime(src) > os.path.getmtime(obj):
            cmd = ([ccache] if ccache else []) + [
                cxx, *flags, *include_flags, "-MMD", "-MF", dep, "-c", src, "-o", obj,
            ]
            subprocess.run(cmd, check=True)
        return obj

    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        test_objs = list(pool.map(compile_test, test_sources))

    link_flags = ["-pthread"]
    if system == "Darwin":
        link_flags.extend(["-Wl,-headerpad_max_install_names", "-liconv", "-lresolv"])
    else:
        link_flags.extend(["-ldl", "-lm"])

    test_bin = os.path.join(test_dir, "rustyget_tests")
    subprocess.run(
        [cxx, *test_objs, archive_path, rust_lib, *openssl_ldflags, *link_flags, "-o", test_bin],
        check=True,
    )

    for td in ("dupematcher1", "dupematcher2", "parchecker", "parcheckerUtf8", "rarrenamer"):
        src_td = os.path.join(root, "tests", "testdata", td)
        dst_td = os.path.join(test_dir, td)
        if not os.path.exists(dst_td):
            shutil.copytree(src_td, dst_td)

    print("Running PostprocessTest...")
    subprocess.run([test_bin, "--run_test=PostprocessTest", "--log_level=message"], cwd=test_dir, check=True)
    print("Running ExitVersionTest...")
    subprocess.run([rustyget_bin, "--version"], cwd=test_dir, check=True)
    print("Running ExitHelpTest...")
    subprocess.run([rustyget_bin, "--help"], stdout=subprocess.DEVNULL, cwd=test_dir, check=True)
    print("Running ConnectTest...")
    subprocess.run([test_bin, "--run_test=ConnectTest", "--log_level=message"], cwd=test_dir, check=True)
    print("All 4 daemon test suites passed!")

# Type-check the Rust crate
check:
    cargo check --manifest-path "{{root_dir}}/rust/Cargo.toml" --all-targets

# Run Clippy on the Rust crate
clippy:
    cargo clippy --manifest-path "{{root_dir}}/rust/Cargo.toml" --lib

# Format the Rust crate
fmt:
    cargo fmt --manifest-path "{{root_dir}}/rust/Cargo.toml"

# Update the local package directory (defaults to ~/Documents/rustyget)
package dest=(env_var("HOME") / "Documents/rustyget") jobs="3": (release jobs)
    mkdir -p "{{dest}}"
    cp "{{root_dir}}/build/release/rustyget" "{{dest}}/rustyget"
    cp "{{root_dir}}/nzbget.conf" "{{dest}}/rustyget.conf.template"
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
        "{{root_dir}}/nzbget.conf" > "{{prefix}}/share/{{package}}/rustyget.conf"

# Uninstall rustyget binary, webui, and docs from `prefix`
uninstall prefix="/usr/local":
    rm -rf "{{prefix}}/share/{{package}}"
    rm -f "{{prefix}}/bin/{{package}}"

# Install rustyget.conf into `prefix/etc`
install-conf prefix="/usr/local":
    mkdir -p "{{prefix}}/etc"
    sed -e "s|^WebDir=.*|WebDir={{prefix}}/share/{{package}}/webui|" \
        -e "s|^ConfigTemplate=.*|ConfigTemplate={{prefix}}/share/{{package}}/rustyget.conf|" \
        "{{root_dir}}/nzbget.conf" > "{{prefix}}/etc/rustyget.conf"

# Remove rustyget.conf from `prefix/etc`
uninstall-conf prefix="/usr/local":
    rm -f "{{prefix}}/etc/rustyget.conf"

# Clean build artifacts
clean:
    rm -rf "{{root_dir}}/build" "{{root_dir}}/rust/target"
