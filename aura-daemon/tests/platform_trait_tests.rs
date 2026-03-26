#[test]
fn test_collect_all_has_no_cfg_macros_in_body() {
    let source = include_str!("../src/collectors/mod.rs");

    let fn_start = source
        .find("pub fn collect_all")
        .expect("collect_all not found");
    let fn_end_rel = source[fn_start..].find("\n}\n").expect("fn end not found");
    let fn_body = &source[fn_start..fn_start + fn_end_rel];

    assert!(
        !fn_body.contains("#[cfg"),
        "collect_all should not contain #[cfg] blocks"
    );
}

#[test]
fn test_all_collector_modules_exist() {
    use aura_daemon::collectors::{cpu, disk, memory, network};

    let _ = (
        cpu::collect,
        memory::collect,
        disk::collect,
        network::collect,
    );
}
