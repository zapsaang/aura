//! Deployment and release contract tests (Todo 15).
//!
//! Parses the checked-in systemd unit, LaunchAgent plist, Home Manager
//! module, Homebrew template, CI/release workflows, and release tooling to
//! lock the deployment alignment contract.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("aura-daemon has a workspace parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"))
}

fn section<'a>(text: &'a str, header: &str) -> &'a str {
    let start = text
        .find(header)
        .unwrap_or_else(|| panic!("missing section {header}"));
    let rest = &text[start + header.len()..];
    match rest.find("\n[") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

fn workflow_job<'a>(workflow: &'a str, job: &str) -> &'a str {
    let marker = format!("\n  {job}:\n");
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("workflow missing job {job}"));
    let body = &workflow[start + marker.len()..];
    let end = body
        .match_indices("\n  ")
        .find_map(|(index, _)| (body.as_bytes().get(index + 3) != Some(&b' ')).then_some(index))
        .unwrap_or(body.len());
    &body[..end]
}

// ---------------------------------------------------------------- systemd

#[test]
fn systemd_service_exact_user_unit_keys() {
    let service = read("deployment/systemd/aura-daemon.service");
    let section = section(&service, "[Service]");
    for key in [
        "Type=notify",
        "NotifyAccess=main",
        "WatchdogSec=3s",
        "RuntimeDirectory=aura",
        "RuntimeDirectoryMode=0700",
    ] {
        assert!(section.contains(key), "service missing {key}");
    }
}

#[test]
fn systemd_service_install_targets_default() {
    let service = read("deployment/systemd/aura-daemon.service");
    let install = section(&service, "[Install]");
    assert!(install.contains("WantedBy=default.target"));
    assert!(!service.contains("User="), "user unit must not set User=");
}

#[test]
fn systemd_service_has_no_forced_shm_path() {
    let service = read("deployment/systemd/aura-daemon.service");
    assert!(!service.contains("AURA_SHM_PATH"));
    assert!(!service.contains("--shm-path"));
}

// ------------------------------------------------------------- LaunchAgent

#[test]
fn plist_label_and_absolute_program() {
    let plist = read("deployment/macos/com.aura.daemon.plist");
    assert!(plist.contains("<key>Label</key>"));
    assert!(plist.contains("<string>com.aura.daemon</string>"));
    let args_start = plist
        .find("<key>ProgramArguments</key>")
        .expect("plist has ProgramArguments");
    let args = &plist[args_start..];
    let first = args
        .split("<string>")
        .nth(1)
        .and_then(|rest| rest.split("</string>").next())
        .expect("plist has a first program argument");
    assert!(
        first.starts_with('/'),
        "daemon path must be absolute: {first}"
    );
    assert!(
        first.ends_with("/aura-daemon"),
        "unexpected program: {first}"
    );
}

#[test]
fn plist_run_at_load_and_keep_alive() {
    let plist = read("deployment/macos/com.aura.daemon.plist");
    assert!(plist.contains("<key>RunAtLoad</key>"));
    assert!(plist.contains("<key>KeepAlive</key>"));
}

#[test]
fn plist_has_no_stale_shm_path_and_keeps_env() {
    let plist = read("deployment/macos/com.aura.daemon.plist");
    assert!(!plist.contains("/tmp/aura_state.dat"));
    assert!(!plist.contains("--shm-path"));
    assert!(plist.contains("<key>RUST_LOG</key>"));
}

// ------------------------------------------------------------- Home Manager

#[test]
fn home_manager_shm_path_null_or_str_default_null() {
    let nix = read("deployment/home-manager/default.nix");
    assert!(nix.contains("lib.types.nullOr lib.types.str"));
    assert!(nix.contains("default = null"));
}

#[test]
fn home_manager_emits_override_only_when_set() {
    let nix = read("deployment/home-manager/default.nix");
    assert!(
        nix.contains("lib.optionals (cfg.shmPath != null)"),
        "launchd args must be conditional on an explicit override"
    );
    assert!(
        nix.contains("lib.optionalString (cfg.shmPath != null)"),
        "systemd ExecStart suffix must be conditional on an explicit override"
    );
}

#[test]
fn home_manager_linux_gpu_feature_darwin_excluded() {
    let nix = read("deployment/home-manager/default.nix");
    assert!(
        nix.contains("lib.optionals pkgs.stdenv.isLinux [ \"aura-daemon/gpu-nvml\" ]"),
        "Linux-only namespaced GPU feature required"
    );
}

#[test]
fn home_manager_version_follows_workspace_manifest() {
    let nix = read("deployment/home-manager/default.nix");
    assert!(nix.contains("builtins.fromTOML (builtins.readFile ../../Cargo.toml)"));
    assert!(nix.contains("workspace.package.version"));
    let manifest = read("Cargo.toml");
    let workspace = section(&manifest, "[workspace.package]");
    let expected = format!("version = \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(workspace.contains(&expected), "workspace version drifted");
}

#[test]
fn home_manager_systemd_user_keys_mirror_unit() {
    let nix = read("deployment/home-manager/default.nix");
    for key in [
        "Type = \"notify\"",
        "NotifyAccess = \"main\"",
        "WatchdogSec = \"3s\"",
        "RuntimeDirectory = \"aura\"",
        "RuntimeDirectoryMode = \"0700\"",
        "WantedBy = [ \"default.target\" ]",
    ] {
        assert!(nix.contains(key), "home-manager missing {key}");
    }
}

// ---------------------------------------------------------------- Homebrew

#[test]
fn homebrew_source_formulas_removed() {
    let dir = workspace_root().join("deployment/homebrew");
    let entries: Vec<String> = std::fs::read_dir(&dir)
        .expect("homebrew deployment dir exists")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries, ["aura.rb.in"], "only the template may remain");
}

#[test]
fn homebrew_template_placeholders_and_binary_only() {
    let template = read("deployment/homebrew/aura.rb.in");
    for placeholder in [
        "{TAG}",
        "{SHA256_LINUX_X86}",
        "{SHA256_LINUX_ARM}",
        "{SHA256_MACOS_ARM}",
        "{SHA256_MACOS_X86}",
    ] {
        assert!(
            template.contains(placeholder),
            "template missing {placeholder}"
        );
    }
    assert!(
        template.contains("license any_of: [\"MIT\", \"Apache-2.0\"]"),
        "SPDX expression must use any_of array form"
    );
    assert!(
        !template.contains("version \""),
        "version is scanned from the URL; explicit version is redundant"
    );
    assert!(template.contains(
        "https://github.com/zapsaang/aura/releases/download/{TAG}/aura-aarch64-apple-darwin.tar.gz"
    ));
    assert!(template.contains(
        "https://github.com/zapsaang/aura/releases/download/{TAG}/aura-x86_64-unknown-linux-gnu.tar.gz"
    ));
    assert!(!template.contains("cargo"), "no source building allowed");
    assert!(!template.contains("depends_on \"rust\""));
    assert!(template.contains("head \"https://github.com/zapsaang/aura.git\", branch: \"main\""));
    assert!(!template.contains("--HEAD"));
}

fn render_script() -> PathBuf {
    workspace_root().join("scripts/render-homebrew-formula.py")
}

const VALID_TAG: &str = "v1.2.3";
const VALID_SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn run_render(args: &[&str]) -> std::process::Output {
    Command::new("python3")
        .arg(render_script())
        .args(args)
        .output()
        .expect("python3 render script runs")
}

fn render_into(tempdir: &Path, name: &str) -> String {
    let out = tempdir.join(name);
    let output = run_render(&[
        "--tag",
        VALID_TAG,
        "--linux-x86",
        VALID_SHA,
        "--linux-arm",
        VALID_SHA,
        "--macos-arm",
        VALID_SHA,
        "--macos-x86",
        VALID_SHA,
        "--out",
        out.to_str().expect("utf8 path"),
    ]);
    assert!(output.status.success(), "render failed: {output:?}");
    std::fs::read_to_string(&out).expect("rendered formula")
}

#[test]
fn homebrew_render_produces_literal_urls_and_digests() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let rendered = render_into(tempdir.path(), "aura.rb");
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
    ] {
        let url = format!(
            "https://github.com/zapsaang/aura/releases/download/{VALID_TAG}/aura-{target}.tar.gz"
        );
        assert!(rendered.contains(&url), "rendered formula missing {url}");
    }
    assert_eq!(rendered.matches(VALID_SHA).count(), 4);
    assert!(rendered.contains("license any_of: [\"MIT\", \"Apache-2.0\"]"));
    assert!(
        !rendered.contains("version \""),
        "rendered formula must not carry an explicit version line"
    );
    for placeholder in [
        "{TAG}",
        "{SHA256_LINUX_X86}",
        "{SHA256_LINUX_ARM}",
        "{SHA256_MACOS_ARM}",
        "{SHA256_MACOS_X86}",
    ] {
        assert!(!rendered.contains(placeholder), "unresolved {placeholder}");
    }
}

#[test]
fn homebrew_render_is_byte_reproducible() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let first = render_into(tempdir.path(), "first.rb");
    let second = render_into(tempdir.path(), "second.rb");
    assert_eq!(first, second, "same inputs must produce identical bytes");
}

#[test]
fn homebrew_render_rejects_bad_tag_and_bad_sha() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let out = tempdir.path().join("aura.rb");
    let out = out.to_str().expect("utf8 path");
    let bad_tag = run_render(&[
        "--tag",
        "1.2.3",
        "--linux-x86",
        VALID_SHA,
        "--linux-arm",
        VALID_SHA,
        "--macos-arm",
        VALID_SHA,
        "--macos-x86",
        VALID_SHA,
        "--out",
        out,
    ]);
    assert!(!bad_tag.status.success(), "non-canonical tag must fail");
    let bad_sha = run_render(&[
        "--tag",
        VALID_TAG,
        "--linux-x86",
        "deadbeef",
        "--linux-arm",
        VALID_SHA,
        "--macos-arm",
        VALID_SHA,
        "--macos-x86",
        VALID_SHA,
        "--out",
        out,
    ]);
    assert!(!bad_sha.status.success(), "short sha must fail");
}

#[test]
fn homebrew_render_rejects_unresolved_placeholder() {
    let snippet = r#"
import importlib.util
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("render_formula", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
try:
    module.render("class X < Formula\n  url \"{UNKNOWN}\"\nend\n", {"TAG": "v1.2.3"})
except module.RenderError:
    sys.exit(0)
sys.exit(1)
"#;
    let output = Command::new("python3")
        .arg("-c")
        .arg(snippet)
        .arg(render_script())
        .output()
        .expect("python3 runs");
    assert!(
        output.status.success(),
        "unknown placeholder must raise RenderError"
    );
}

// ------------------------------------------------------------------ CI/CD

#[test]
fn ci_workflow_pins_185_and_msrv_170_on_both_hosts() {
    let ci = read(".github/workflows/ci.yml");
    assert!(ci.contains("toolchain: '1.85.0'"));
    assert!(ci.contains("toolchain: '1.70.0'"));
    assert!(ci.contains("ubuntu-24.04"));
    assert!(ci.contains("macos-15"));
    assert!(ci.contains("cargo +1.85.0 check --workspace --all-features --locked"));
}

#[test]
fn release_workflow_exact_four_tar_gz_targets() {
    let release = read(".github/workflows/release.yml");
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        let archive = format!("aura-{target}.tar.gz");
        assert!(release.contains(&archive), "release missing {archive}");
    }
    assert!(
        !release.contains(".tar.xz"),
        "stale xz packaging must be gone"
    );
    assert_eq!(
        release.matches("actions/upload-artifact@").count(),
        17,
        "four archives, formula, nine lanes, and three producers"
    );
    let registry = read("qa/compliance-qa-registry.json");
    assert_eq!(
        registry.matches("scripts/package-release.py").count(),
        4,
        "the closed registry assigns every release package command"
    );
}

#[test]
fn release_workflow_gpu_feature_linux_only() {
    let release = read(".github/workflows/release.yml");
    assert_eq!(
        release.matches("--feature aura-daemon/gpu-nvml").count(),
        3,
        "the two Linux release lanes and Ubuntu GPU lane enable NVML"
    );
    for linux in [
        workflow_job(&release, "release-linux-x86"),
        workflow_job(&release, "release-linux-arm64"),
    ] {
        assert!(linux.contains("--feature aura-daemon/gpu-nvml"));
    }
    let darwin_arm = workflow_job(&release, "release-macos-arm64");
    let darwin_x86 = workflow_job(&release, "release-macos-x86");
    for darwin in [darwin_arm, darwin_x86] {
        assert!(
            !darwin.contains("gpu-nvml"),
            "Darwin builds must not enable NVML"
        );
    }
    let registry = read("qa/compliance-qa-registry.json");
    assert_eq!(
        registry
            .matches("MACOSX_DEPLOYMENT_TARGET=12.0 cargo +1.85.0 build")
            .count(),
        2,
        "both Darwin build commands retain the deployment target"
    );
}

#[test]
fn release_workflow_homebrew_render_audit_ordering() {
    let release = read(".github/workflows/release.yml");
    let render = workflow_job(&release, "homebrew-render");
    for lane in [
        "release-linux-x86",
        "release-linux-arm64",
        "release-macos-arm64",
        "release-macos-x86",
    ] {
        assert!(render.contains(lane), "render must need {lane}");
    }
    assert!(render.contains("--job ubuntu-default"));
    let audit = workflow_job(&release, "homebrew-audit");
    assert!(audit.contains("homebrew-render"));
    assert!(audit.contains("--job macos-default"));
    let registry = read("qa/compliance-qa-registry.json");
    assert!(registry.contains("scripts/render-homebrew-formula.py"));
    assert!(registry.contains("ruby -c dist/homebrew/aura.rb"));
    assert!(registry.contains("brew audit --strict --formula aura/audit/aura"));
    assert!(!registry.contains("brew audit --strict --formula dist/homebrew/aura.rb"));
    assert!(!release.contains("publish-homebrew"));
    assert!(!release.contains("gh release"));
    assert!(!release.contains("homebrew-tap"));
}

#[test]
fn release_workflow_keeps_intel_macos_coverage() {
    let release = read(".github/workflows/release.yml");
    let x86 = workflow_job(&release, "release-macos-x86");
    assert!(x86.contains("x86_64-apple-darwin"));
    assert!(x86.contains("--job release-macos-x86"));
    let registry = read("qa/compliance-qa-registry.json");
    for proof in [
        "release-macos-x86-file",
        "release-macos-x86-minos",
        "release-macos-x86-imports",
    ] {
        assert!(
            registry.contains(proof),
            "Intel macOS lane registry missing {proof}"
        );
    }
}

// ----------------------------------------------------------------- scripts

#[test]
fn release_scripts_and_template_are_checked_in() {
    for path in [
        "scripts/package-release.py",
        "scripts/verify-release-binary.py",
        "scripts/smoke-release.py",
        "scripts/render-homebrew-formula.py",
        "deployment/homebrew/aura.rb.in",
    ] {
        let full = workspace_root().join(path);
        let metadata =
            std::fs::metadata(&full).unwrap_or_else(|error| panic!("missing {path}: {error}"));
        assert!(
            metadata.is_file() && metadata.len() > 0,
            "{path} must be a nonempty file"
        );
    }
}
