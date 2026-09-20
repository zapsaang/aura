//! Todo 2 runtime path resolution contract: lexical byte validation,
//! Darwin alias normalization, Linux base selection, and trusted
//! descriptor-relative resolution.

#![cfg(unix)]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aura_common::runtime::path::validate;
use aura_common::runtime::{linux_default_base, normalize_darwin_aliases, RuntimeLocation};
use aura_common::AuraError;

fn security_reason(err: AuraError) -> String {
    match err {
        AuraError::Security(reason) => reason,
        other => panic!("expected Security, got {other:?}"),
    }
}

fn reject_reason(path: &str) -> String {
    security_reason(validate(OsStr::new(path)).expect_err("path must be rejected"))
}

#[test]
fn accepts_clean_absolute_path() {
    let path = validate(OsStr::new("/tmp/aura-1000/state.dat")).unwrap();
    assert_eq!(path, Path::new("/tmp/aura-1000/state.dat"));
}

#[test]
fn rejects_relative_path() {
    assert_eq!(reject_reason("tmp/state.dat"), "path is not absolute");
}

#[test]
fn rejects_empty_path() {
    assert_eq!(reject_reason(""), "path is not absolute");
}

#[test]
fn rejects_nul_byte() {
    assert_eq!(reject_reason("/tmp/a\0b"), "path contains NUL byte");
}

#[test]
fn rejects_non_utf8() {
    let raw = OsStr::from_bytes(b"/tmp/aura-\xff/state.dat");
    assert_eq!(
        security_reason(validate(raw).expect_err("non-UTF-8 must be rejected")),
        "path is not valid UTF-8"
    );
}

#[test]
fn rejects_empty_interior_component() {
    assert_eq!(
        reject_reason("/tmp//state.dat"),
        "path contains forbidden component "
    );
}

#[test]
fn rejects_trailing_slash() {
    assert_eq!(
        reject_reason("/tmp/aura/"),
        "path contains forbidden component "
    );
}

#[test]
fn rejects_dot_component() {
    assert_eq!(
        reject_reason("/tmp/./state.dat"),
        "path contains forbidden component ."
    );
}

#[test]
fn rejects_dotdot_component() {
    assert_eq!(
        reject_reason("/tmp/../state.dat"),
        "path contains forbidden component .."
    );
}

#[test]
fn rejects_newline_with_exact_reason() {
    assert_eq!(
        reject_reason("/tmp/a\nb"),
        "path contains control character U+000A"
    );
}

#[test]
fn rejects_carriage_return_with_exact_reason() {
    assert_eq!(
        reject_reason("/tmp/a\rb"),
        "path contains control character U+000D"
    );
}

#[test]
fn rejects_tab_with_exact_reason() {
    assert_eq!(
        reject_reason("/tmp/a\tb"),
        "path contains control character U+0009"
    );
}

#[test]
fn rejects_escape_with_exact_reason() {
    assert_eq!(
        reject_reason("/tmp/a\u{1b}b"),
        "path contains control character U+001B"
    );
}

#[test]
fn rejects_delete_control() {
    assert_eq!(
        reject_reason("/tmp/a\u{7f}b"),
        "path contains control character U+007F"
    );
}

#[test]
fn rejects_c1_control() {
    assert_eq!(
        reject_reason("/tmp/a\u{85}b"),
        "path contains control character U+0085"
    );
}

#[test]
fn precedence_nul_beats_control() {
    assert_eq!(reject_reason("/tmp/a\0\n"), "path contains NUL byte");
}

#[test]
fn precedence_nul_beats_non_utf8() {
    let raw = OsStr::from_bytes(b"/\xff\0");
    assert_eq!(
        security_reason(validate(raw).expect_err("must reject")),
        "path contains NUL byte"
    );
}

#[test]
fn precedence_non_utf8_beats_component() {
    let raw = OsStr::from_bytes(b"/\xff/../x");
    assert_eq!(
        security_reason(validate(raw).expect_err("must reject")),
        "path is not valid UTF-8"
    );
}

#[test]
fn precedence_component_beats_control() {
    assert_eq!(
        reject_reason("/a/./b\n"),
        "path contains forbidden component ."
    );
}

#[test]
fn reports_first_control_scalar_in_path_order() {
    assert_eq!(
        reject_reason("/a\u{b}x/b\ny"),
        "path contains control character U+000B"
    );
}

#[test]
fn control_reason_never_contains_hostile_text() {
    let reason = reject_reason("/tmp/evil\npath");
    assert_eq!(reason, "path contains control character U+000A");
    assert!(!reason.contains("evil"));
}

#[test]
fn accepts_non_control_unicode() {
    validate(OsStr::new("/tmp/aura-é/state.dat")).unwrap();
}

#[test]
fn normalize_rewrites_var_prefix() {
    assert_eq!(
        normalize_darwin_aliases("/var/folders/xy/T/"),
        "/private/var/folders/xy/T/"
    );
}

#[test]
fn normalize_rewrites_bare_tmp() {
    assert_eq!(normalize_darwin_aliases("/tmp"), "/private/tmp");
}

#[test]
fn normalize_rewrites_tmp_slash() {
    assert_eq!(normalize_darwin_aliases("/tmp/"), "/private/tmp/");
}

#[test]
fn normalize_rewrites_tmp_child() {
    assert_eq!(normalize_darwin_aliases("/tmp/aura"), "/private/tmp/aura");
}

#[test]
fn normalize_leaves_other_paths_unchanged() {
    assert_eq!(normalize_darwin_aliases("/var"), "/var");
    assert_eq!(normalize_darwin_aliases("/vartmp/x"), "/vartmp/x");
    assert_eq!(normalize_darwin_aliases("/private/tmp"), "/private/tmp");
    assert_eq!(normalize_darwin_aliases("tmp"), "tmp");
}

#[test]
fn linux_base_prefers_run_user() {
    assert_eq!(
        linux_default_base(true, 1000),
        (PathBuf::from("/run/user/1000"), "aura".to_string())
    );
}

#[test]
fn linux_base_falls_back_only_when_uid_dir_absent() {
    assert_eq!(
        linux_default_base(false, 1000),
        (PathBuf::from("/tmp"), "aura-1000".to_string())
    );
}

fn test_dir(tag: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aura-runtime-path-{tag}-{}-{ts}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    std::fs::canonicalize(&dir).unwrap()
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn override_resolution_derives_leaf_names() {
    let dir = test_dir("override-ok");
    let location = RuntimeLocation::resolve_override(dir.join("custom.dat").as_os_str()).unwrap();
    assert_eq!(location.state_name(), "custom.dat");
    assert_eq!(location.lock_name(), "custom.dat.lock");
    assert_eq!(location.temp_prefix(), ".custom.dat.tmp-");
    assert!(!location.is_default());
    assert_eq!(location.state_path(), dir.join("custom.dat"));
    cleanup(&dir);
}

#[test]
fn override_missing_parent_is_exact_reason() {
    let dir = test_dir("override-missing");
    let missing = dir.join("absent");
    let err = RuntimeLocation::resolve_override(missing.join("state.dat").as_os_str())
        .expect_err("missing parent must be rejected");
    assert_eq!(security_reason(err), "override parent is not trusted");
    cleanup(&dir);
}

#[test]
fn override_parent_with_wrong_mode_is_rejected() {
    let dir = test_dir("override-mode");
    let parent = dir.join("loose");
    std::fs::DirBuilder::new()
        .mode(0o755)
        .create(&parent)
        .unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = RuntimeLocation::resolve_override(parent.join("state.dat").as_os_str())
        .expect_err("loose parent must be rejected");
    let reason = security_reason(err);
    assert!(
        reason.contains("expected mode 0700, found 0755"),
        "unexpected reason: {reason}"
    );
    cleanup(&dir);
}

#[test]
fn override_symlink_ancestor_is_rejected() {
    let dir = test_dir("override-symlink");
    let real = dir.join("real");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&real)
        .unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = RuntimeLocation::resolve_override(link.join("state.dat").as_os_str())
        .expect_err("symlink ancestor must be rejected");
    assert_eq!(security_reason(err), "component link: symlink not allowed");
    cleanup(&dir);
}

#[test]
fn default_resolution_creates_child_0700_with_exact_names() {
    let dir = test_dir("default-create");
    let location = RuntimeLocation::resolve_default_under(&dir, "aura", true).unwrap();
    assert!(location.is_default());
    assert_eq!(location.state_name(), "state.dat");
    assert_eq!(location.lock_name(), "state.lock");
    assert_eq!(location.temp_prefix(), ".state.dat.tmp-");
    let mode = std::fs::metadata(dir.join("aura"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o700,
        "AURA child must be exactly 0700, got {mode:04o}"
    );
    cleanup(&dir);
}

#[test]
fn default_resolution_accepts_existing_child() {
    let dir = test_dir("default-existing");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(dir.join("aura"))
        .unwrap();
    RuntimeLocation::resolve_default_under(&dir, "aura", true).unwrap();
    RuntimeLocation::resolve_default_under(&dir, "aura", false).unwrap();
    cleanup(&dir);
}

#[test]
fn default_resolution_missing_child_without_create_is_offline() {
    let dir = test_dir("default-offline");
    match RuntimeLocation::resolve_default_under(&dir, "aura", false) {
        Err(AuraError::Offline(_)) => {}
        other => panic!("expected Offline, got {other:?}"),
    }
    assert!(!dir.join("aura").exists(), "CLI resolution creates nothing");
    cleanup(&dir);
}

#[test]
fn default_resolution_rejects_wrong_mode_child_without_mutation() {
    let dir = test_dir("default-child-mode");
    let child = dir.join("aura");
    std::fs::DirBuilder::new()
        .mode(0o755)
        .create(&child)
        .unwrap();
    std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = RuntimeLocation::resolve_default_under(&dir, "aura", true)
        .expect_err("wrong-mode child must be rejected");
    assert!(security_reason(err).contains("expected mode 0700, found 0755"));
    let mode = std::fs::metadata(&child).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "existing child must never be chmodded");
    cleanup(&dir);
}

#[test]
fn default_resolution_rejects_symlink_child() {
    let dir = test_dir("default-child-symlink");
    let real = dir.join("real");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&real)
        .unwrap();
    std::os::unix::fs::symlink(&real, dir.join("aura")).unwrap();
    let err = RuntimeLocation::resolve_default_under(&dir, "aura", true)
        .expect_err("symlink child must be rejected");
    assert_eq!(security_reason(err), "component aura: symlink not allowed");
    cleanup(&dir);
}
