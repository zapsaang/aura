use std::ffi::OsStr;
use std::fs;
use std::mem::{align_of, size_of};
use std::path::{Path, PathBuf};

use aura_common::TelemetryArchive;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("aura-common must have a workspace parent")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read source directory") {
        let path = entry.expect("read source entry").path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension() == Some(OsStr::new("rs")) {
            files.push(path);
        }
    }
}

fn production_sources() -> Vec<(PathBuf, String)> {
    let root = workspace_root();
    let mut files = Vec::new();
    for package in ["aura-common", "aura-daemon", "aura-cli"] {
        collect_rs_files(&root.join(package).join("src"), &mut files);
        let build = root.join(package).join("build.rs");
        if build.is_file() {
            files.push(build);
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).expect("production Rust must be UTF-8");
            (path, source)
        })
        .collect()
}

fn assert_absent(needle: &str) {
    let offenders: Vec<PathBuf> = production_sources()
        .into_iter()
        .filter_map(|(path, source)| source.contains(needle).then_some(path))
        .collect();
    assert!(offenders.is_empty(), "found `{needle}` in {offenders:?}");
}

#[test]
fn production_source_inventory_is_not_empty() {
    assert!(production_sources().len() >= 30);
}

#[test]
fn manifests_declare_rust_1_70() {
    let root = workspace_root();
    let workspace = fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    assert!(workspace.contains("rust-version = \"1.70\""));
    for package in ["aura-common", "aura-daemon", "aura-cli"] {
        let manifest =
            fs::read_to_string(root.join(package).join("Cargo.toml")).expect("package manifest");
        assert!(manifest.contains("rust-version.workspace = true"));
    }
}

#[test]
fn unsigned_is_multiple_of_is_not_used() {
    assert_absent("is_multiple_of");
}

#[test]
fn offset_of_macro_is_not_used() {
    assert_absent("offset_of!");
}

#[test]
fn atomic_from_ptr_is_not_used() {
    assert_absent("AtomicU64::from_ptr");
}

#[test]
fn unsafe_extern_blocks_are_not_used() {
    assert_absent("unsafe extern");
}

#[test]
fn post_1_70_lazy_primitives_are_not_used() {
    assert_absent("LazyLock");
    assert_absent("LazyCell");
}

#[test]
fn post_1_70_pointer_helpers_are_not_used() {
    assert_absent("ptr::from_ref");
    assert_absent("ptr::from_mut");
}

#[test]
fn post_1_70_filesystem_helpers_are_not_used() {
    assert_absent("std::fs::exists");
    assert_absent("std::path::absolute");
}

#[test]
fn post_1_70_error_helper_is_not_used() {
    assert_absent("Error::other");
}

#[test]
fn c_string_literal_syntax_is_not_used() {
    assert_absent("c\"");
}

#[allow(clippy::assertions_on_constants)]
#[test]
fn target_is_little_endian() {
    assert!(cfg!(target_endian = "little"));
}

#[allow(clippy::assertions_on_constants)]
#[test]
fn target_has_native_64_bit_atomics() {
    assert!(cfg!(target_has_atomic = "64"));
}

#[test]
fn archive_atomic_copy_invariants_hold() {
    assert_eq!(size_of::<TelemetryArchive>(), 65_536);
    assert_eq!(size_of::<TelemetryArchive>() % 8, 0);
    assert_eq!(align_of::<TelemetryArchive>(), 8);
}

#[test]
fn aligned_atomic_helpers_are_private_and_directional() {
    let source = fs::read_to_string(workspace_root().join("aura-common/src/double_buffer.rs"))
        .expect("double-buffer source");
    assert!(source.contains("unsafe fn load_aligned_atomic_u64(src: *const u64) -> u64"));
    assert!(source.contains("unsafe fn store_aligned_atomic_u64(dst: *mut u64, value: u64)"));
    assert!(source.contains("let atomic = src.cast::<AtomicU64>();"));
    assert!(source.contains("let atomic = dst.cast::<AtomicU64>();"));
}

#[test]
fn darwin_ffi_uses_plain_extern_block() {
    let source = fs::read_to_string(workspace_root().join("aura-daemon/src/platform/macos/ffi.rs"))
        .expect("Darwin FFI source");
    assert!(source.contains("extern \"C\" {"));
    assert!(!source.contains("unsafe extern"));
}

#[test]
fn gate_launchers_disable_bytecode_before_importing_local_modules() {
    let root = workspace_root();
    for launcher in [
        "scripts/verify-prerequisite-gates.py",
        "scripts/verify-tip-gates.py",
    ] {
        let source = fs::read_to_string(root.join(launcher)).expect("gate launcher source");
        let disable = source
            .find("sys.dont_write_bytecode = True")
            .expect("gate launcher must disable bytecode generation");
        let local_import = source
            .find("from gate_worktree import")
            .expect("gate launcher must import shared implementation");
        assert!(
            disable < local_import,
            "{launcher} disables bytecode too late"
        );
    }
}
