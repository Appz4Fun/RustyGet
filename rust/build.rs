//! Builds the CPU tier check object that the `rustyget` binary links.
//!
//! A release build sets its CPU tier with `-C target-cpu` for every crate.
//! The tier check (`tiercheck.rs`) must run on CPUs below that tier, so it is
//! compiled here on its own, for the target's default CPU, with the tier
//! passed as `--cfg rustyget_tier="..."`, and linked into the binary.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `-C target-cpu` value in the flags cargo hands rustc, if any.
fn target_cpu() -> Option<String> {
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let flags: Vec<&str> = flags.split('\x1f').collect();
    let mut cpu = None;
    for (i, f) in flags.iter().enumerate() {
        let opt = if *f == "-C" {
            flags.get(i + 1).copied()
        } else {
            f.strip_prefix("-C")
        };
        if let Some(v) = opt.and_then(|o| o.strip_prefix("target-cpu=")) {
            cpu = Some(v.to_string());
        }
    }
    cpu
}

fn main() {
    println!("cargo:rerun-if-changed=tiercheck.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    bundle_webui(&out);
    let target = env::var("TARGET").expect("TARGET");
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let obj = out.join("rustyget_tiercheck.o");
    let mut cmd = Command::new(rustc);
    cmd.args([
        "--edition=2021",
        "--crate-name",
        "rustyget_tiercheck",
        "--crate-type",
        "lib",
        "--emit=obj",
        "-C",
        "panic=abort",
        "-C",
        "opt-level=2",
        "-C",
        "codegen-units=1",
        "-C",
        "debuginfo=0",
        "--check-cfg",
        "cfg(rustyget_tier, values(any()))",
        "--target",
        &target,
    ]);
    if let Some(cpu) = target_cpu().filter(|c| c != "generic") {
        cmd.arg("--cfg").arg(format!("rustyget_tier=\"{cpu}\""));
    }
    cmd.arg("tiercheck.rs").arg("-o").arg(&obj);
    // a wrapper's or the build's own flags (target-cpu among them) stay out
    cmd.env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    let status = cmd.status().expect("run rustc for tiercheck.rs");
    assert!(status.success(), "compiling tiercheck.rs failed");
    println!("cargo:rustc-link-arg-bins={}", obj.display());
    if env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        // room to rewrite load commands after linking, as the C++ link did
        println!("cargo:rustc-link-arg-bins=-Wl,-headerpad_max_install_names");
    }
}

/// Collects the files under `dir` (relative names with `/`), sorted.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        let rel = format!("{prefix}{name}");
        if path.is_dir() {
            collect(&path, &format!("{rel}/"), out);
        } else if path.is_file() && !name.to_ascii_lowercase().contains(".conf") {
            out.push((rel, path));
        }
    }
}

/// Builds the web interface into the binary (`$OUT_DIR/webui_bundle.rs`), so
/// it is there when WebDir is empty or cannot be read. A tree without
/// `webui/` builds an empty bundle.
fn bundle_webui(out: &Path) {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let webui = manifest.join("..").join("webui");
    println!("cargo:rerun-if-changed={}", webui.display());
    let mut files = Vec::new();
    collect(&webui, "", &mut files);
    let mut code = String::from("pub(crate) static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, path) in &files {
        let abs = path.canonicalize().unwrap_or_else(|_| path.clone());
        code.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", abs.display().to_string()));
    }
    code.push_str("];\n");
    std::fs::write(out.join("webui_bundle.rs"), code).expect("write webui_bundle.rs");
}
