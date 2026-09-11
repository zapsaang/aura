//! Documentation contract (Todo 16): tracked baseline docs, ADR coverage of
//! the locked decisions, stale-claim negative fixtures, PLOC rule
//! documentation, and fact-labeled remediation appendices.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|error| panic!("read {rel}: {error}"))
}

fn gitignore_lines() -> Vec<String> {
    read(".gitignore")
        .lines()
        .map(|line| line.trim().to_string())
        .collect()
}

fn readme() -> String {
    read("README.md")
}

fn project() -> String {
    read("docs/project.md")
}

fn blueprint() -> String {
    read("docs/tech_blueprint.md")
}

fn adr() -> String {
    read("docs/adr.md")
}

// --- .gitignore contract -------------------------------------------------

#[test]
fn gitignore_has_no_markdown_blanket() {
    assert!(
        !gitignore_lines().iter().any(|line| line == "*.md"),
        "blanket *.md ignore must be removed (AUD-014)"
    );
}

#[test]
fn gitignore_retains_omo_private_ignore() {
    assert!(
        gitignore_lines().iter().any(|line| line == "/.omo/"),
        "/.omo/ private ignore must be retained"
    );
}

#[test]
fn gitignore_retains_narrow_private_ignores() {
    let lines = gitignore_lines();
    assert!(lines.iter().any(|line| line == "shell.nix"));
    assert!(lines.iter().any(|line| line == "opencode.json"));
}

// --- Baseline docs --------------------------------------------------------

#[test]
fn baseline_docs_exist_and_nonempty() {
    for rel in [
        "README.md",
        "docs/design-compliance-audit-2026-08-30.md",
        "docs/project.md",
        "docs/tech_blueprint.md",
    ] {
        let text = read(rel);
        assert!(text.len() > 512, "{rel} must be a substantive tracked doc");
    }
}

// --- ADR coverage ----------------------------------------------------------

#[test]
fn adr_records_all_nine_decisions() {
    let adr = adr();
    for index in 1..=9 {
        assert!(
            adr.contains(&format!("ADR-00{index}")),
            "adr.md must record ADR-00{index}"
        );
    }
}

#[test]
fn adr_records_abi_v2() {
    let adr = adr();
    assert!(adr.contains("ABI v2"));
    assert!(adr.contains("ARCHIVE_VERSION = 2"));
}

#[test]
fn adr_records_per_buffer_seqlock() {
    let adr = adr();
    assert!(adr.contains("Per-buffer SeqLock"));
    assert!(adr.contains("seq[2]"));
}

#[test]
fn adr_records_read_deadlines() {
    let adr = adr();
    assert!(adr.contains("SEQLOCK_RETRY_ADMISSION_MS = 10"));
    assert!(adr.contains("MAX_SPIN_WAIT_MS = 100"));
}

#[test]
fn adr_records_shm_mode_0600() {
    assert!(adr().contains("0o600"));
}

#[test]
fn adr_records_crc_scope() {
    let adr = adr();
    assert!(adr.contains("integrity"));
    assert!(adr.contains("authentication"));
}

#[test]
fn adr_records_macos_public_api_only() {
    let adr = adr();
    assert!(adr.contains("publi\u{63}"));
    assert!(adr.contains("unsupported"));
}

#[test]
fn adr_records_linux_nvml_runtime_loading() {
    let adr = adr();
    assert!(adr.contains("libnvidia-ml.so.1"));
    assert!(adr.contains("runtime"));
}

#[test]
fn adr_records_daemon_derivation() {
    let adr = adr();
    assert!(adr.contains("DerivedStats"));
    assert!(adr.contains("presentation"));
}

#[test]
fn adr_records_no_v1_compatibility() {
    let adr = adr();
    assert!(adr.contains("no v1"));
    assert!(adr.contains("expected 2, found"));
}

#[test]
fn adr_records_process_storage_history() {
    let adr = adr();
    assert!(adr.contains("4c419f\u{63}"));
    assert!(adr.contains("Process and Storage"));
}

// --- README command surface -------------------------------------------------

#[test]
fn readme_documents_all_modules() {
    let readme = readme();
    for module in [
        "-m cpu",
        "-m pro\u{63}",
        "-m mem",
        "-m swap",
        "-m disk",
        "-m net",
        "-m os",
        "-m gpu",
        "-m all",
    ] {
        assert!(readme.contains(module), "README must document `{module}`");
    }
}

#[test]
fn readme_documents_formats_and_color_modes() {
    let readme = readme();
    for token in ["human", "json", "value", "ansi", "tmux", "zellij", "none"] {
        assert!(readme.contains(token), "README must document `{token}`");
    }
}

#[test]
fn readme_documents_cli_and_daemon_flags() {
    let readme = readme();
    for flag in [
        "--module",
        "--format",
        "--color",
        "--shm-path",
        "--heartbeat-ms",
    ] {
        assert!(readme.contains(flag), "README must document `{flag}`");
    }
}

// --- Stale-claim negative fixtures -------------------------------------------

#[test]
fn docs_have_no_stale_fixed_shm_path() {
    for text in [readme(), project(), blueprint(), adr()] {
        assert!(
            !text.contains("aura_state.dat"),
            "stale fixed shm path must not appear in docs"
        );
    }
}

#[test]
fn readme_has_no_stale_homebrew_tap_install() {
    let readme = readme();
    assert!(!readme.contains("brew install zapsaang/tap/aura"));
    assert!(!readme.contains("brew tap zapsaang/tap"));
}

#[test]
fn blueprint_records_per_buffer_seqlock() {
    let blueprint = blueprint();
    assert!(blueprint.contains("seq[2]"));
    assert!(blueprint.contains("active_index"));
    assert!(
        !blueprint.contains("AtomicUsize"),
        "superseded single-counter design must not remain as current text"
    );
}

#[test]
fn docs_have_no_world_readable_shm_claim() {
    for text in [readme(), project(), blueprint()] {
        assert!(!text.contains("0o666"));
        assert!(!text.contains("0666"));
    }
}

// --- Behavioral contract documentation ----------------------------------------

#[test]
fn offline_contract_documented() {
    let readme = readme();
    assert!(readme.contains("[AURA: OFFLINE]"));
    assert!(readme.contains("2 seconds"));
    assert!(blueprint().contains("[AURA: OFFLINE]"));
}

#[test]
fn macos_gpu_unsupported_documented() {
    assert!(readme().contains("unsupported"));
    assert!(project().contains("不支持"));
}

// --- PLOC documentation --------------------------------------------------------

#[test]
fn ploc_rules_documented() {
    let ploc = read("docs/ploc.md");
    for token in [
        "250",
        "cfg(test)",
        "Raw strings",
        "Byte strings",
        "lifetimes",
        "nesting",
        "symlink",
        "benches",
        "fixtures",
        "target",
    ] {
        assert!(ploc.contains(token), "ploc.md must document `{token}`");
    }
}

// --- Remediation appendices -----------------------------------------------------

#[test]
fn remediation_appendices_are_fact_labeled() {
    let project = project();
    assert!(project.contains("事实"));
    assert!(project.contains("AUD-001"));
    let blueprint = blueprint();
    assert!(blueprint.contains("事实"));
    assert!(blueprint.contains("ADR-002"));
}

// --- PLOC oversize negative fixture ------------------------------------------------

#[test]
fn ploc_checker_rejects_oversize_file() {
    let root = std::env::temp_dir().join(format!("aura-ploc-oversize-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create oversize fixture root");
    let mut source = String::new();
    for index in 0..300 {
        source.push_str(&format!("let v{index} = {index};\n"));
    }
    fs::write(root.join("lib.rs"), source).expect("write oversize fixture");
    let output = std::process::Command::new("python3")
        .arg(repo_root().join("scripts").join("check-rust-loc.py"))
        .arg("--root")
        .arg(&root)
        .output()
        .expect("run check-rust-loc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&root);
    assert!(
        !output.status.success(),
        "oversize fixture must fail: {stderr}"
    );
    assert!(
        stderr.contains("exceeds 250"),
        "stderr must report the ceiling: {stderr}"
    );
}
