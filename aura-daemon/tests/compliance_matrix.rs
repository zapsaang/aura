//! AUD-001..=015 design-compliance regression matrix (Todo 17, registry t17-01).
//!
//! Exactly fifteen tests, one per locked audit row. Each test re-validates
//! its row against the checked-in `fixtures/compliance-matrix.json` and then
//! exercises the underlying production code paths directly (never a string
//! existence check alone). The matrix is the post-gate-fresh evidence for
//! AUD-015: every referenced prior-todo suite file must exist on disk.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod support;

use std::fs;
use std::path::{Path, PathBuf};

use aura_common::{
    AuraError, TelemetryArchive, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_META_WALLCLOCK, MAX_NETIFS,
};
use aura_daemon::collectors::FixedCollectorState;
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::{Finalizer, Heartbeat};

// -----------------------------------------------------------------------
// Locked matrix table (normative; mirrored byte-for-byte in fixtures/).
// -----------------------------------------------------------------------

struct Row {
    aud: &'static str,
    verdict: &'static str,
    todos: &'static [&'static str],
    assertions: &'static [&'static str],
    evidence: &'static [&'static str],
}

const MATRIX: [Row; 15] = [
    Row {
        aud: "AUD-001",
        verdict: "confirmed; magic-close partial only",
        todos: &["5", "6", "15"],
        assertions: &[
            "deploy.user_watchdog",
            "notify.failed_cycle_no_ping",
            "notify.no_hardware",
            "notify.ready_after_publish",
        ],
        evidence: &["t05-01", "t06-01", "t15-01"],
    },
    Row {
        aud: "AUD-002",
        verdict: "confirmed",
        todos: &["3"],
        assertions: &["network.cap_twice"],
        evidence: &["t03-01"],
    },
    Row {
        aud: "AUD-003",
        verdict: "confirmed missing Process/Storage",
        todos: &["8", "9"],
        assertions: &[
            "platform.macos_unsupported_truth",
            "process.linux_populates",
            "storage.linux_populates",
        ],
        evidence: &["t08-01", "t09-01", "t10-01"],
    },
    Row {
        aud: "AUD-004",
        verdict: "confirmed",
        todos: &["5"],
        assertions: &["transaction.rollback_all"],
        evidence: &["t05-01"],
    },
    Row {
        aud: "AUD-005",
        verdict: "confirmed",
        todos: &["13"],
        assertions: &["output.seven_keys_counts_null"],
        evidence: &["t13-01"],
    },
    Row {
        aud: "AUD-006",
        verdict: "packaging true; universal NVML false",
        todos: &["12", "15"],
        assertions: &["deploy.apple_no_nvml", "gpu.linux_dynamic_degrade"],
        evidence: &["t12-01", "t15-01"],
    },
    Row {
        aud: "AUD-007",
        verdict: "confirmed",
        todos: &["5", "7"],
        assertions: &["derived.warm_delta"],
        evidence: &["t05-01", "t07-01"],
    },
    Row {
        aud: "AUD-008",
        verdict: "gaps true; stable parity impossible",
        todos: &["9", "10", "11"],
        assertions: &["macos.public_capability_table"],
        evidence: &["t09-01", "t10-01", "t11-01-macos", "t11-01-ubuntu"],
    },
    Row {
        aud: "AUD-009",
        verdict: "protocol issues true; generic weak-memory unproven",
        todos: &["4", "14"],
        assertions: &["errors.classification", "seqlock.protocol_crash_deadline"],
        evidence: &["t04-01", "t04-03", "t14-01"],
    },
    Row {
        aud: "AUD-010",
        verdict: "confirmed",
        todos: &["2", "15"],
        assertions: &["runtime.same_path_secure"],
        evidence: &["t02-01", "t15-01"],
    },
    Row {
        aud: "AUD-011",
        verdict: "confirmed",
        todos: &["1", "7", "13"],
        assertions: &["derived.daemon_owned", "output.no_derivation"],
        evidence: &["t01-06", "t07-01", "t13-01"],
    },
    Row {
        aud: "AUD-012",
        verdict: "confirmed",
        todos: &["1", "11"],
        assertions: &["abi.meta_fields", "meta.dual_clock"],
        evidence: &["t01-06", "t11-01-macos", "t11-01-ubuntu"],
    },
    Row {
        aud: "AUD-013",
        verdict: "confirmed; CRC auth premise rejected",
        todos: &["1", "2"],
        assertions: &["abi.crc_corruption_only", "security.no_follow_modes"],
        evidence: &["t01-06", "t02-02"],
    },
    Row {
        aud: "AUD-014",
        verdict: "confirmed and stronger",
        todos: &["preflight", "16"],
        assertions: &["docs.tracked_source_manifest"],
        evidence: &["t16-01", "t16-04"],
    },
    Row {
        aud: "AUD-015",
        verdict: "coverage verdict confirmed; historical PASS stale",
        todos: &["all", "17"],
        assertions: &["evidence.post_gate_fresh"],
        evidence: &["t17-01", "t17-02", "t17-03", "tip-t17"],
    },
];

/// File-backed evidence suites keyed by registry ID. Command-only IDs
/// resolve to the file whose content they gate (`t16-04` guards the audit
/// document's trackedness; tip gates are the verifier script).
const EVIDENCE_FILES: [(&str, &str); 24] = [
    ("t01-06", "aura-common/tests/abi_v2.rs"),
    ("t02-01", "aura-common/tests/runtime_path.rs"),
    ("t02-02", "aura-daemon/tests/shm_security.rs"),
    ("t03-01", "aura-daemon/tests/network_limits.rs"),
    ("t04-01", "aura-common/tests/double_buffer_protocol.rs"),
    ("t04-03", "aura-daemon/tests/ipc_concurrent.rs"),
    ("t05-01", "aura-daemon/tests/collector_transaction.rs"),
    ("t06-01", "aura-daemon/tests/systemd_notify.rs"),
    ("t07-01", "aura-daemon/tests/derived_metrics.rs"),
    ("t08-01", "aura-daemon/tests/process_collector.rs"),
    ("t09-01", "aura-daemon/tests/storage_collector.rs"),
    ("t10-01", "aura-daemon/tests/macos_contract.rs"),
    ("t11-01-macos", "aura-daemon/tests/meta_contract.rs"),
    ("t11-01-ubuntu", "aura-daemon/tests/meta_contract.rs"),
    ("t12-01", "aura-daemon/tests/gpu_contract.rs"),
    ("t13-01", "aura-cli/tests/output_contract.rs"),
    ("t14-01", "aura-cli/tests/error_contract.rs"),
    ("t15-01", "aura-daemon/tests/deployment_contract.rs"),
    ("t16-01", "aura-common/tests/documentation_contract.rs"),
    ("t16-04", "docs/design-compliance-audit-2026-08-30.md"),
    ("t17-01", "aura-daemon/tests/compliance_matrix.rs"),
    ("t17-02", "aura-daemon/tests/allocation_contract.rs"),
    ("t17-03", "aura-daemon/tests/ipc_multiprocess.rs"),
    ("tip-t17", "scripts/verify-tip-gates.py"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read_repo(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
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

fn fixture_matrix() -> serde_json::Value {
    let raw = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/compliance-matrix.json"
    ))
    .expect("read compliance-matrix.json");
    assert!(raw.ends_with("}\n"), "fixture is compact JSON plus LF");
    serde_json::from_str(&raw).expect("fixture parses")
}

/// Cross-check one locked row against the checked-in fixture. The fixture is
/// the machine-readable matrix consumed by the final aggregate; drift between
/// the code table and the fixture fails here, never downstream.
fn check_row(row: &Row) {
    normative_binding();
    let fixture = fixture_matrix();
    assert_eq!(fixture["matrix_version"], serde_json::json!(1));
    let audits = fixture["audits"].as_array().expect("audits array");
    assert_eq!(audits.len(), 15, "fixture row count drift");
    let entry = audits
        .iter()
        .find(|entry| entry["aud"] == row.aud)
        .unwrap_or_else(|| panic!("fixture missing {}", row.aud));
    let assertions: Vec<&str> = entry["assertions"]
        .as_array()
        .expect("assertions array")
        .iter()
        .map(|value| value.as_str().expect("string assertion"))
        .collect();
    assert_eq!(assertions, row.assertions, "{} assertion drift", row.aud);
    let evidence: Vec<&str> = entry["evidence"]
        .as_array()
        .expect("evidence array")
        .iter()
        .map(|value| value.as_str().expect("string evidence"))
        .collect();
    assert_eq!(evidence, row.evidence, "{} evidence drift", row.aud);
    assert_eq!(
        entry["verdict"].as_str().expect("verdict"),
        row.verdict,
        "{} verdict drift",
        row.aud
    );
    let todos: Vec<String> = entry["todos"]
        .as_array()
        .expect("todos array")
        .iter()
        .map(|value| match value {
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::String(text) => text.clone(),
            other => panic!("bad todos entry {other}"),
        })
        .collect();
    assert_eq!(todos, row.todos, "{} todos drift", row.aud);
    for id in row.evidence {
        assert!(
            EVIDENCE_FILES.iter().any(|(candidate, _)| candidate == id),
            "evidence id {id} lacks a file mapping"
        );
    }
}

fn row(index: usize) -> &'static Row {
    &MATRIX[index]
}

/// Anchors the Rust matrix to the normative QA contract and registry under
/// qa/ — the independent sources the aggregate gate consumes. The fixture
/// mirrors this file's own table, so without this binding the matrix would
/// only prove agreement with itself.
fn normative_binding() {
    let contract: serde_json::Value =
        serde_json::from_str(&read_repo("qa/f1-compliance-contract.json"))
            .expect("contract parses");
    assert_eq!(contract["schema_version"], serde_json::json!(1));
    let checks = contract["checks"].as_array().expect("checks array");
    assert_eq!(checks.len(), 39, "contract check count drift");
    let ids: Vec<&str> = checks
        .iter()
        .map(|check| check["id"].as_str().expect("check id"))
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ids, "contract checks not sorted by id");
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "contract check ids duplicate");

    let audits: Vec<&serde_json::Value> = checks
        .iter()
        .filter(|check| check["id"].as_str().expect("check id").starts_with("AUD-"))
        .collect();
    let audit_ids: Vec<String> = audits
        .iter()
        .map(|check| check["id"].as_str().expect("aud id").to_string())
        .collect();
    let expected: Vec<String> = (1..=15).map(|n| format!("AUD-{n:03}")).collect();
    assert_eq!(audit_ids, expected, "contract AUD coverage drift");

    for row in MATRIX.iter() {
        let check = audits
            .iter()
            .find(|check| check["id"].as_str().expect("aud id") == row.aud)
            .unwrap_or_else(|| panic!("contract missing {}", row.aud));
        let registry_ids: Vec<&str> = check["registry_ids"]
            .as_array()
            .expect("registry_ids array")
            .iter()
            .map(|value| value.as_str().expect("string registry id"))
            .collect();
        let mut sorted_ids = registry_ids.clone();
        sorted_ids.sort_unstable();
        assert_eq!(
            sorted_ids, registry_ids,
            "{} registry_ids not sorted",
            row.aud
        );
        sorted_ids.dedup();
        assert_eq!(
            sorted_ids.len(),
            registry_ids.len(),
            "{} registry_ids duplicate",
            row.aud
        );
        assert!(
            registry_ids.contains(&"t17-01"),
            "{} must reference this suite (t17-01)",
            row.aud
        );
        // Binding: registry_ids equal the row's evidence unioned with t17-01
        // (this suite), which the Rust table lists only on the post-gate
        // AUD-015 row. Exact equality cannot hold for AUD-001..=014 because
        // their evidence deliberately omits the self-reference.
        let mut union: Vec<&str> = row.evidence.to_vec();
        union.push("t17-01");
        union.sort_unstable();
        union.dedup();
        assert_eq!(
            union, registry_ids,
            "{} evidence/registry binding drift",
            row.aud
        );
        let aggregate_paths: Vec<&str> = check["aggregate_paths"]
            .as_array()
            .expect("aggregate_paths array")
            .iter()
            .map(|value| value.as_str().expect("string aggregate path"))
            .collect();
        let mut sorted_paths = aggregate_paths.clone();
        sorted_paths.sort_unstable();
        assert_eq!(
            sorted_paths, aggregate_paths,
            "{} aggregate_paths not sorted",
            row.aud
        );
        sorted_paths.dedup();
        assert_eq!(
            sorted_paths.len(),
            aggregate_paths.len(),
            "{} aggregate_paths duplicate",
            row.aud
        );
        assert!(
            aggregate_paths.contains(&"source/compliance-matrix.json"),
            "{} must aggregate the source matrix",
            row.aud
        );
    }

    let registry: serde_json::Value =
        serde_json::from_str(&read_repo("qa/compliance-qa-registry.json"))
            .expect("registry parses");
    assert_eq!(registry["schema_version"], serde_json::json!(1));
    let commands = registry["commands"].as_array().expect("commands array");
    assert_eq!(commands.len(), 114, "registry command count drift");
    let mut command_ids: Vec<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    command_ids.sort_unstable();
    command_ids.dedup();
    assert_eq!(
        command_ids.len(),
        commands.len(),
        "registry command ids duplicate"
    );
    let t17 = commands
        .iter()
        .find(|command| command["id"] == "t17-01")
        .expect("registry has t17-01");
    assert_eq!(
        t17["command"].as_str().expect("command string"),
        "cargo test -p aura-daemon --test compliance_matrix --locked -- --test-threads=1",
        "t17-01 command drift"
    );
}

// -----------------------------------------------------------------------
// Shared scripted clock for finalizer-driven rows.
// -----------------------------------------------------------------------

struct FixedClock {
    monotonic_ns: u64,
    wallclock_ns: u64,
}

impl Clock for FixedClock {
    fn sample(&mut self) -> aura_common::AuraResult<ClockSample> {
        Ok(ClockSample {
            monotonic_ns: self.monotonic_ns,
            wallclock_ns: self.wallclock_ns,
        })
    }
}

fn heartbeat() -> Heartbeat {
    Heartbeat::from_millis(100).expect("positive heartbeat")
}

// -----------------------------------------------------------------------
// AUD-001 — watchdog is systemd-notify only (todos 5, 6, 15)
// -----------------------------------------------------------------------

#[test]
fn aud001_notify_watchdog_deploy_contract() {
    check_row(row(0));

    // notify.no_hardware: no daemon production source touches /dev/watchdog.
    for source in [
        include_str!("../src/heartbeat.rs"),
        include_str!("../src/notify/mod.rs"),
        include_str!("../src/lifecycle.rs"),
        include_str!("../src/daemon.rs"),
    ] {
        assert!(!source.contains("/dev/watchdog"), "hardware watchdog path");
        assert!(!source.contains("WDIOC"), "hardware watchdog ioctl");
    }
    #[cfg(target_os = "linux")]
    {
        let linux_notify = include_str!("../src/notify/linux.rs");
        assert!(!linux_notify.contains("/dev/watchdog"));
        assert!(!linux_notify.contains("WDIOC"));
    }

    // notify.ready_after_publish: when the READY notification fails, the
    // finalized archive is already published — notification follows publish.
    #[cfg(target_os = "linux")]
    {
        use support::transaction::{lifecycle, Failure};

        let mut lifecycle = lifecycle(Failure::None, Some(1));
        lifecycle.warm_up(heartbeat()).expect("warm-up");
        let error = lifecycle.cycle().expect_err("READY failure is fatal");
        assert!(
            matches!(error, AuraError::Fatal(_)),
            "notify failure class: {error:?}"
        );
        assert_eq!(lifecycle.publisher().calls, 1, "publish preceded READY");
        let published = lifecycle.publisher().published();
        assert_eq!(published.meta.uptime_secs, 12, "finalized bytes visible");
        assert_eq!(published.checksum, published.calculate_checksum());
        assert!(published.capabilities & aura_common::CAP_META_UPTIME != 0);
    }

    // notify.failed_cycle_no_ping: a failed cycle after READY sends nothing.
    #[cfg(target_os = "linux")]
    {
        use support::transaction::{lifecycle, Failure};

        let mut lifecycle = lifecycle(Failure::None, None);
        lifecycle.warm_up(heartbeat()).expect("warm-up");
        lifecycle.cycle().expect("first published cycle");
        assert_eq!(lifecycle.notifier().calls, 1, "exactly READY sent");
        lifecycle.collector_mut().failure = Failure::Collector;
        assert!(lifecycle.cycle().is_err(), "collector fault fails cycle");
        assert_eq!(lifecycle.notifier().calls, 1, "failed cycle sent no ping");
        assert_eq!(lifecycle.publisher().calls, 1, "failed cycle unpublished");
    }

    // deploy.user_watchdog: checked-in systemd user unit is Type=notify with
    // WatchdogSec and no forced shm path; Home Manager mirrors it.
    let service = read_repo("deployment/systemd/aura-daemon.service");
    assert!(service.contains("Type=notify"), "service notify type");
    assert!(
        service.contains("WatchdogSec=3s"),
        "service watchdog window"
    );
    assert!(
        service.contains("WantedBy=default.target"),
        "user unit target"
    );
    assert!(!service.contains("/dev/watchdog"), "no hardware watchdog");
    let home_manager = read_repo("deployment/home-manager/default.nix");
    assert!(
        home_manager.contains("Type = \"notify\""),
        "home-manager notify type"
    );
    assert!(
        home_manager.contains("WatchdogSec = \"3s\""),
        "home-manager watchdog window"
    );
}

// -----------------------------------------------------------------------
// AUD-002 — MAX_NETIFS cap is stable across repeated cycles (todo 3)
// -----------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn over_cap_net_dev(interfaces: usize) -> Vec<u8> {
    let mut content = String::from(
        "Inter-|   Receive                                                |  Transmit\n \
         face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets\n",
    );
    for index in 0..interfaces {
        content.push_str(&format!(
            "  eth{index}: {rx} 100 0 0 0 0 0 0 {tx} 200 0 0 0 0 0 0\n",
            rx = 1_000_000 + index,
            tx = 2_000_000 + index
        ));
    }
    content.into_bytes()
}

#[test]
#[cfg(target_os = "linux")]
fn aud002_network_cap_twice_stable() {
    check_row(row(1));

    // network.cap_twice: the production parser caps twice in a row without
    // panic and with byte-identical retained prefixes (AUD-002 abort class).
    use aura_daemon::collectors::network::linux::parse_net_dev;

    let input = over_cap_net_dev(MAX_NETIFS + 2);
    let mut first = TelemetryArchive::zeroed().network;
    let mut second = TelemetryArchive::zeroed().network;
    parse_net_dev(&input, &mut first).expect("first over-cap parse");
    parse_net_dev(&input, &mut second).expect("second over-cap parse");
    assert_eq!(first.if_count as usize, MAX_NETIFS, "capped at MAX_NETIFS");
    assert_eq!(second.if_count as usize, MAX_NETIFS, "second cycle capped");
    assert_eq!(first.truncated, 1, "truncation reported");
    assert_eq!(second.truncated, 1, "truncation reported twice");
    for index in 0..MAX_NETIFS {
        assert_eq!(first.interfaces[index].rx_bytes, 1_000_000 + index as u64);
        assert_eq!(
            first.interfaces[index].rx_bytes, second.interfaces[index].rx_bytes,
            "cycle {index} prefix stable"
        );
        assert_eq!(
            first.interfaces[index].name, second.interfaces[index].name,
            "name prefix stable"
        );
    }
}

// -----------------------------------------------------------------------
// AUD-003 — Process/Storage restored on Linux; macOS truthfully unsupported
// -----------------------------------------------------------------------

#[test]
#[cfg(target_os = "linux")]
fn aud003_process_storage_populated_platform_truth() {
    check_row(row(2));

    // process.linux_populates: real collector over a synthetic proc root.
    use aura_daemon::collectors::process::linux::{collect, ProcessScan};
    use aura_daemon::collectors::process::state::ProcessBaseline;
    use std::os::unix::ffi::OsStrExt;

    let root = tempfile::tempdir().expect("proc root");
    let stat = b"4242 (aura-test) R 1 4242 4242 0 -1 4194304 100 0 0 0 40 10 0 0 20 0 1 0 7 200 1 1 1 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0";
    fs::create_dir(root.path().join("4242")).expect("pid dir");
    fs::write(root.path().join("4242/stat"), stat).expect("pid stat");
    let sleeping = b"777 (aura-idle) S 1 777 777 0 -1 4194304 50 0 0 0 5 5 0 0 20 0 1 0 8 200 1 1 1 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0";
    fs::create_dir(root.path().join("777")).expect("pid dir");
    fs::write(root.path().join("777/stat"), sleeping).expect("pid stat");

    let mut baseline = ProcessBaseline::default();
    let mut out = TelemetryArchive::zeroed().process;
    let mut stat_buf = Vec::with_capacity(4096);
    let mut path_buf = Vec::with_capacity(4096);
    let stat_v2 = b"4242 (aura-test) R 1 4242 4242 0 -1 4194304 100 0 0 0 90 20 0 0 20 0 1 0 7 200 1 1 1 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0";
    for (delta, content) in [(0u64, &stat[..]), (100u64, &stat_v2[..])] {
        fs::write(root.path().join("4242/stat"), content).expect("pid stat");
        let mut scan = ProcessScan {
            proc_root: root.path().as_os_str().as_bytes(),
            page_size: 4096,
            online_cores: 1,
            delta_global_ticks: delta,
            stat_buf: &mut stat_buf,
            path_buf: &mut path_buf,
        };
        collect(&mut scan, &mut baseline, &mut out).expect("process collect");
    }
    assert_eq!(out.total, 2, "both fixtures counted");
    assert_eq!(out.running, 1, "R state counted");
    assert_eq!(out.sleeping, 1, "S state counted");
    assert_eq!(out.flags & aura_common::PROCESS_TRUNCATED, 0);
    assert!(
        out.top_cpu_count >= 1,
        "warm delta produced a cpu candidate"
    );
    assert_eq!(out.top_cpu[0].pid, 4242);

    // storage.linux_populates: production diskstats + mountinfo parsers.
    use aura_daemon::collectors::storage::linux::{parse_diskstats, parse_mountinfo};
    use aura_daemon::collectors::storage::state::DiskRawSnapshot;
    use aura_daemon::collectors::storage::FsCapacity;

    let mut storage = TelemetryArchive::zeroed().storage;
    let mut raw = [DiskRawSnapshot::zero(); aura_common::MAX_DISKS];
    parse_diskstats(
        include_bytes!("fixtures/proc_diskstats_sample.txt"),
        &mut storage,
        &mut raw,
    )
    .expect("parse diskstats");
    assert_eq!(storage.disk_count, 7, "fixture disks populated");
    assert_eq!(storage.disks[0].name.as_str(), "sda");
    assert_eq!(storage.disks[0].read_bytes, 20480 * 512);
    parse_mountinfo(
        include_bytes!("fixtures/proc_mountinfo_sample.txt"),
        &mut storage,
        &mut |_path: &[u8]| {
            Some(FsCapacity {
                blocks: 1_000,
                bfree: 500,
                bavail: 250,
                unit: 1,
            })
        },
    )
    .expect("parse mountinfo");
    assert!(storage.mount_count > 0, "fixture mounts populated");
    assert_eq!(storage.disk_truncated, 0);
    assert!(
        storage.mount_count as usize <= aura_common::MAX_MOUNTS,
        "mount cap respected"
    );
    assert!(
        !storage.mounts[0].mountpoint.iter().all(|byte| *byte == 0),
        "first mount has a mountpoint"
    );

    // platform.macos_unsupported_truth: the macOS production sources cannot
    // claim Process/Storage/GPU support — they never set those capability
    // bits and publish zeroed records instead.
    let process_macos = include_str!("../src/collectors/process/macos.rs");
    assert!(!process_macos.contains("CAP_PROCESS"), "no process caps");
    assert!(process_macos.contains("total: 0"), "zeroed process table");
    let storage_macos = include_str!("../src/collectors/storage/macos.rs");
    assert!(!storage_macos.contains("CAP_STORAGE_DISK"), "no disk caps");
    assert!(
        storage_macos.contains("disk_count = 0"),
        "zeroed disk table"
    );
    let platform_macos = include_str!("../src/collectors/meta/macos.rs");
    assert!(!platform_macos.contains("CAP_GPU"), "no gpu caps from meta");
    let gpu = include_str!("../src/collectors/gpu.rs");
    assert!(
        gpu.contains("cfg(not(all(feature = \"gpu-nvml\", target_os = \"linux\")))"),
        "non-Linux NVML is compiled to the zeroing fallback"
    );
}

// -----------------------------------------------------------------------
// AUD-004 — failed cycle rolls back publication entirely (todo 5)
// -----------------------------------------------------------------------

#[test]
#[cfg(target_os = "linux")]
fn aud004_transaction_rollback_all() {
    check_row(row(3));

    // transaction.rollback_all: collector, finalizer, and publisher faults
    // each leave the committed state and the published bytes untouched.
    use support::transaction::{assert_fixed_state_eq, lifecycle, Failure};

    for failure in [Failure::Collector, Failure::Finalizer, Failure::Publisher] {
        let mut lifecycle = lifecycle(failure, None);
        let committed_before = lifecycle.state().committed().clone();
        let published_before = lifecycle.publisher().raw_bytes();
        assert!(
            lifecycle.cycle().is_err(),
            "{failure:?} cycle must fail the boundary"
        );
        assert_fixed_state_eq(lifecycle.state().committed(), &committed_before);
        assert_eq!(
            lifecycle.publisher().raw_bytes(),
            published_before,
            "{failure:?} must not alter published bytes"
        );
        assert_eq!(lifecycle.notifier().calls, 0, "{failure:?} notified");
    }
}

// -----------------------------------------------------------------------
// AUD-005 — JSON root: seven capability-gated dimensions, null counts
// -----------------------------------------------------------------------

#[test]
fn aud005_output_seven_keys_counts_null() {
    check_row(row(4));

    // output.seven_keys_counts_null: an all-zero (unsupported) archive renders
    // version 2, a capability map, and exactly seven dimension objects whose
    // leaf values are JSON null rather than fabricated zeros.
    let mut archive = TelemetryArchive::zeroed();
    archive.version = aura_common::ARCHIVE_VERSION;
    let rendered = aura_cli::format::json::render(aura_cli::args::Module::All, &archive)
        .expect("render all modules");
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
    let root = value.as_object().expect("root object");
    let keys: Vec<&str> = root.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "capabilities",
            "cpu",
            "gpu",
            "memory",
            "meta",
            "network",
            "process",
            "storage",
            "version"
        ],
        "root key set drift"
    );
    assert_eq!(value["version"], serde_json::json!(2), "ABI version two");
    for dimension in [
        "cpu", "process", "memory", "storage", "network", "meta", "gpu",
    ] {
        let object = value[dimension].as_object().expect("dimension object");
        assert!(!object.is_empty(), "{dimension} renders its schema");
        for (leaf, leaf_value) in object {
            if leaf == "os" || (dimension == "meta" && leaf == "timestamp_ns") {
                continue;
            }
            assert!(
                leaf_value.is_null() || leaf_value.is_object(),
                "{dimension}.{leaf} must be null when unsupported, got {leaf_value}"
            );
        }
    }
    assert_eq!(
        value["meta"]["timestamp_ns"],
        serde_json::json!(0),
        "producer timestamp renders verbatim even when meta is unsupported"
    );
    assert!(value["process"]["total"].is_null(), "process count null");
    assert!(value["process"]["top_cpu"].is_null(), "process top null");
    assert!(value["storage"]["disks"].is_null(), "storage disks null");
    let capabilities = value["capabilities"].as_object().expect("capability map");
    assert_eq!(capabilities.len(), 33, "capability key count");
    assert!(
        capabilities.values().all(|flag| flag == false),
        "unsupported archive reports every capability false"
    );
}

// -----------------------------------------------------------------------
// AUD-006 — Linux dynamic NVML degrades; Apple artifacts never enable it
// -----------------------------------------------------------------------

#[test]
fn aud006_gpu_dynamic_degrade_apple_no_nvml() {
    check_row(row(5));

    // gpu.linux_dynamic_degrade: on this host (no NVML driver, default
    // features) the production entry points degrade to a zeroed table with
    // nvml_available clear and never fail the cycle.
    let mut gpu = TelemetryArchive::zeroed().gpu;
    aura_daemon::collectors::gpu::init_nvml(&mut gpu).expect("init degrades");
    aura_daemon::collectors::gpu::collect_nvml(&mut gpu).expect("collect degrades");
    assert_eq!(gpu.nvml_available, 0, "nvml reported unavailable");
    assert_eq!(gpu.gpu_count, 0, "no fabricated devices");
    assert!(
        gpu.gpus.iter().all(|record| record.capabilities == 0),
        "no fabricated record capabilities"
    );
    assert_eq!(
        aura_common::NVML_LIBRARY,
        "libnvidia-ml.so.1",
        "runtime-loaded soname"
    );

    // deploy.apple_no_nvml: Home Manager enables the feature on Linux only,
    // and the release workflow enables it only for the two Linux targets.
    let home_manager = read_repo("deployment/home-manager/default.nix");
    assert!(
        home_manager.contains(
            "buildFeatures = lib.optionals pkgs.stdenv.isLinux [ \"aura-daemon/gpu-nvml\" ];"
        ),
        "home-manager gates gpu-nvml on isLinux"
    );
    let release = read_repo(".github/workflows/release.yml");
    for job in ["release-linux-x86", "release-linux-arm64"] {
        assert!(
            workflow_job(&release, job).contains("aura-daemon/gpu-nvml"),
            "{job} must enable gpu-nvml"
        );
    }
    for job in ["release-macos-arm64", "release-macos-x86"] {
        assert!(
            !workflow_job(&release, job).contains("gpu-nvml"),
            "{job} must not enable gpu-nvml"
        );
    }
    assert!(
        release.contains("--target aarch64-apple-darwin"),
        "apple arm target present"
    );
    assert!(
        release.contains("--target x86_64-apple-darwin"),
        "apple x86 target present"
    );
}

// -----------------------------------------------------------------------
// AUD-007 — warm baseline: interval deltas, no first-publish spike
// -----------------------------------------------------------------------

#[test]
fn aud007_derived_warm_delta() {
    check_row(row(6));

    // derived.warm_delta: first finalize with zero baselines reseeds without a
    // lifetime spike; the second finalize reports the interval share.
    let mut state = FixedCollectorState::default();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE;
    state.archive.cpu.user_ticks = 100;
    state.archive.cpu.system_ticks = 50;
    state.archive.cpu.idle_ticks = 850;
    state.archive.cpu.total_ticks = 1_000;
    state.archive.cpu.core_count = 1;
    state.archive.cpu.cores[0].core_index = 0;
    state.archive.cpu.cores[0].user_ticks = 100;
    state.archive.cpu.cores[0].system_ticks = 50;
    state.archive.cpu.cores[0].idle_ticks = 850;
    state.archive.cpu.cores[0].total_ticks = 1_000;

    let first_clock = FixedClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_700_000_000_000_000_000,
    };
    let mut first = SystemFinalizer::new(first_clock);
    first.finalize(&mut state).expect("first finalize");
    assert_eq!(state.archive.cpu.usage_percent, 0.0, "no lifetime spike");
    assert_eq!(state.archive.cpu.cores[0].usage_percent, 0.0);

    state.archive.cpu.user_ticks = 200;
    state.archive.cpu.system_ticks = 100;
    state.archive.cpu.idle_ticks = 1_700;
    state.archive.cpu.total_ticks = 2_000;
    state.archive.cpu.cores[0].user_ticks = 200;
    state.archive.cpu.cores[0].system_ticks = 100;
    state.archive.cpu.cores[0].idle_ticks = 1_700;
    state.archive.cpu.cores[0].total_ticks = 2_000;
    let second_clock = FixedClock {
        monotonic_ns: 2_000_000_000,
        wallclock_ns: 1_700_000_001_000_000_000,
    };
    let mut second = SystemFinalizer::new(second_clock);
    second.finalize(&mut state).expect("second finalize");
    assert_eq!(
        state.archive.cpu.usage_percent, 15.0,
        "interval share (1000-850)/1000"
    );
    assert_eq!(state.archive.cpu.cores[0].usage_percent, 15.0);
}

// -----------------------------------------------------------------------
// AUD-008 — public capability table matches the macOS truth
// -----------------------------------------------------------------------

#[test]
fn aud008_macos_public_capability_table() {
    check_row(row(7));

    // macos.public_capability_table: the README's public table states the
    // macOS boundary (capability-gated public APIs, GPU unsupported), the
    // audit records the gap list, and the ABI capability constants cover
    // every gapped dimension so "zero" is distinguishable from "unsupported".
    let readme = read_repo("README.md");
    let table = readme
        .find("## Platform Support")
        .expect("public capability table section");
    let table = &readme[table..];
    assert!(
        table.contains("| CPU, Process, Memory, Storage, Network, Meta | yes | capability-gated (public APIs only) |"),
        "public macOS capability row"
    );
    assert!(
        table.contains("unsupported"),
        "GPU row marks macOS unsupported"
    );
    assert!(
        table.contains("render as\n`N/A`") || table.contains("N/A"),
        "unsupported renders as N/A"
    );
    let gap_bits = std::hint::black_box([
        aura_common::CAP_META_OS_VERSION,
        aura_common::CAP_MEMORY_SWAP,
        aura_common::CAP_NETWORK_RATES,
        aura_common::CAP_GPU_ENUMERATION,
    ]);
    assert!(
        gap_bits.iter().all(|bit| *bit != 0),
        "gap dimensions carry public capability bits"
    );
    let audit = read_repo("docs/design-compliance-audit-2026-08-30.md");
    assert!(
        audit.contains("AUD-008"),
        "audit finding remains linked for the gap list"
    );
}

// -----------------------------------------------------------------------
// AUD-009 — SeqLock protocol: crash window, retry-admission deadline, errors
// -----------------------------------------------------------------------

#[test]
#[cfg(target_os = "linux")]
fn aud009_seqlock_protocol_and_error_classification() {
    check_row(row(8));

    use aura_common::{read_double_buffer_with_elapsed, SHM_SIZE};
    use memmap2::MmapOptions;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let directory = tempfile::tempdir().expect("shm dir");
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("private shm dir");
    let path = directory.path().join("state.dat");

    let archive = |generation: u64| {
        let mut archive = TelemetryArchive::zeroed();
        archive.version = aura_common::ARCHIVE_VERSION;
        archive.meta.timestamp_ns = generation;
        archive.cpu.total_ticks = generation;
        archive.checksum = archive.calculate_checksum();
        archive
    };

    let mut writer = aura_daemon::state::ShmHandle::new(&path).expect("writer");
    writer.write(&archive(1)).expect("seed snapshot");

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open state");
    // SAFETY: the state file is exactly SHM_SIZE, page-aligned by mmap.
    let mut injection = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    // SAFETY: header words live at offset 0 (active) and 8..24 (seq pair),
    // naturally aligned; single-threaded here.
    let active = unsafe { *injection.as_ptr().cast::<u64>() };
    let seq_offset = 8 + (active as usize) * 8;
    unsafe {
        (*injection.as_mut_ptr().add(seq_offset).cast::<AtomicU64>()).store(3, Ordering::Release)
    };
    drop(injection);

    let reader_map = {
        let read_file = std::fs::File::open(&path).expect("open read");
        // SAFETY: read-only mapping of the exact SHM extent.
        unsafe { MmapOptions::new().len(SHM_SIZE).map(&read_file).unwrap() }
    };

    // seqlock.protocol_crash_deadline: an abandoned odd sequence (writer
    // crash mid-publication) is never admitted; the retry-admission deadline
    // bounds the wait deterministically and classifies as SeqLockTimeout.
    let mut tick = 0u64;
    let result = unsafe {
        read_double_buffer_with_elapsed(reader_map.as_ptr(), || {
            tick += 1;
            if tick <= 1 {
                Duration::ZERO
            } else {
                Duration::from_millis(11)
            }
        })
    };
    match result {
        Err(AuraError::SeqLockTimeout) => {}
        other => panic!("odd sequence must exhaust the deadline: {other:?}"),
    }

    // Recovery: the next production write completes the protocol and the
    // reader observes exactly the new generation, never a mixed archive.
    writer.write(&archive(2)).expect("recovery write");
    let snapshot =
        unsafe { aura_common::read_double_buffer(reader_map.as_ptr()) }.expect("recovered read");
    assert_eq!(snapshot.meta.timestamp_ns, 2);
    assert_eq!(snapshot.meta.timestamp_ns, snapshot.cpu.total_ticks);
    assert_eq!(snapshot.checksum, snapshot.calculate_checksum());

    // errors.classification: the offline/timeout/checksum/security classes
    // are distinct user-visible categories with stable display prefixes.
    let classes = [
        AuraError::NotPublished.to_string(),
        AuraError::SeqLockTimeout.to_string(),
        AuraError::ChecksumMismatch {
            expected: 1,
            actual: 2,
        }
        .to_string(),
        AuraError::InvalidShmHeader { found: 7 }.to_string(),
        AuraError::StaleData {
            age_ms: 3_000,
            threshold_ms: 2_000,
        }
        .to_string(),
        AuraError::Offline("x".to_string()).to_string(),
        AuraError::Security("x".to_string()).to_string(),
    ];
    for (index, text) in classes.iter().enumerate() {
        assert!(
            classes[index + 1..].iter().all(|other| other != text),
            "error class texts must be distinct: {text}"
        );
    }
    assert!(classes[5].starts_with("daemon is offline"), "offline class");
    assert!(classes[2].contains("checksum mismatch"), "checksum class");

    // Missing state on the CLI reader path classifies as Offline, not a
    // generic error (AUD-009 offline degradation): the default-location
    // probe maps an absent child dir or state leaf to the offline class.
    let missing_dir = tempfile::tempdir().expect("empty parent");
    std::fs::set_permissions(
        missing_dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("private dir");
    match aura_cli::reader::TelemetryReader::new_default_under(missing_dir.path(), "aura") {
        Err(AuraError::Offline(_)) => {}
        other => panic!("missing default shm must classify Offline: {other:?}"),
    }
}

// -----------------------------------------------------------------------
// AUD-010 — daemon and CLI resolve the same secure runtime path
// -----------------------------------------------------------------------

#[test]
#[cfg(target_os = "linux")]
fn aud010_runtime_same_path_secure() {
    check_row(row(9));

    use aura_common::runtime::RuntimeLocation;
    use std::os::unix::fs::PermissionsExt;

    let parent = tempfile::tempdir().expect("runtime parent");
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).expect("private parent");

    // runtime.same_path_secure: the daemon (create) and CLI (open-only)
    // resolve byte-identical state/lock/temp names beneath one private 0700
    // directory; the reader opens exactly what the writer published.
    let daemon_location =
        RuntimeLocation::resolve_default_under(parent.path(), "rt", true).expect("daemon resolve");
    let cli_location =
        RuntimeLocation::resolve_default_under(parent.path(), "rt", false).expect("cli resolve");
    assert_eq!(daemon_location.state_path(), cli_location.state_path());
    assert_eq!(daemon_location.state_name(), "state.dat");
    assert_eq!(cli_location.lock_name(), "state.lock");
    assert_eq!(daemon_location.temp_prefix(), ".state.dat.tmp-");
    let mode = fs::metadata(daemon_location.state_path().parent().unwrap())
        .expect("runtime dir")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "runtime dir is private");

    let mut handle =
        aura_daemon::state::ShmHandle::new(&daemon_location.state_path()).expect("daemon open");
    let mut archive = TelemetryArchive::zeroed();
    archive.version = aura_common::ARCHIVE_VERSION;
    archive.meta.timestamp_ns = aura_common::monotonic_ns();
    archive.checksum = archive.calculate_checksum();
    handle.write(&archive).expect("publish");
    let reader = aura_cli::reader::TelemetryReader::new(&cli_location.state_path())
        .expect("cli open same path");
    let snapshot = reader.read().expect("read same path");
    assert_eq!(snapshot.meta.timestamp_ns, archive.meta.timestamp_ns);

    // Path hygiene: relative and dot-component overrides are rejected as
    // Security before any filesystem touch.
    for raw in ["relative/state.dat", "/tmp/./state.dat", "/tmp//state.dat"] {
        match aura_common::runtime::path::validate(std::ffi::OsStr::new(raw)) {
            Err(AuraError::Security(_)) => {}
            other => panic!("override {raw} must be a Security rejection: {other:?}"),
        }
    }
}

// -----------------------------------------------------------------------
// AUD-011 — derived metrics are daemon-owned; the CLI never derives
// -----------------------------------------------------------------------

#[test]
fn aud011_derived_daemon_owned_cli_never_derives() {
    check_row(row(10));

    // derived.daemon_owned: the daemon finalize pass computes derived
    // percentages/tones into the archive's DerivedStats.
    let mut state = FixedCollectorState::default();
    state.archive.capabilities = aura_common::CAP_MEMORY_RAM_TOTAL
        | aura_common::CAP_MEMORY_RAM_USED
        | aura_common::CAP_MEMORY_SWAP;
    state.archive.memory.ram_total = 200;
    state.archive.memory.ram_used = 50;
    state.archive.memory.swap_total = 100;
    state.archive.memory.swap_used = 25;
    let clock = FixedClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_700_000_000_000_000_000,
    };
    let mut finalizer = SystemFinalizer::new(clock);
    finalizer.finalize(&mut state).expect("finalize derives");
    assert_eq!(state.archive.derived.ram_used_percent, 25.0);
    assert_eq!(state.archive.derived.swap_used_percent, 25.0);
    assert!(
        state.archive.derived.ram_tone <= aura_common::TONE_MAX,
        "tone byte in range"
    );

    // output.no_derivation: the CLI renders the daemon-computed derived
    // fields verbatim even when raw counters contradict them — it never
    // recomputes percentages from raw fields.
    let mut archive = state.archive;
    // Mutate the raw counters so the raw-implied percent (100/200 = 50.0%)
    // contradicts the stored derived value (25.0%): a CLI that recomputed
    // percentages from raw fields would now emit "50.0%" and fail below.
    archive.memory.ram_used = 100;
    archive.checksum = archive.calculate_checksum();
    assert_eq!(archive.memory.ram_total, 200);
    assert_eq!(archive.memory.ram_used, 100);
    assert_eq!(
        archive.derived.ram_used_percent, 25.0,
        "derived field untouched by raw mutation"
    );
    let value = aura_cli::output::value::render(aura_cli::args::Module::Mem, &archive);
    assert_eq!(value, "mem=25.0%", "value renderer uses derived field");
    let human = aura_cli::output::render(
        aura_cli::args::Module::Mem,
        aura_cli::args::ColorMode::None,
        &archive,
    );
    assert!(
        human.contains("25.0%"),
        "human renderer uses derived field: {human}"
    );
    assert!(
        !human.contains("50.0%"),
        "raw-implied percent must not leak into output: {human}"
    );
}

// -----------------------------------------------------------------------
// AUD-012 — ABI meta fields and dual clocks
// -----------------------------------------------------------------------

#[test]
#[cfg(target_os = "linux")]
fn aud012_abi_meta_fields_dual_clock() {
    check_row(row(11));

    // abi.meta_fields: the production os-release parser fills version and
    // version_codename as first-class ABI fields.
    use aura_daemon::collectors::meta::linux::parse_os_release;

    let os_release = b"ID=auraos\nVERSION=\"1.0 (core)\"\nVERSION_ID=\"1.0\"\nVERSION_CODENAME=core\nPRETTY_NAME=\"Aura OS 1.0\"\n";
    let fingerprint = parse_os_release(os_release).expect("parse");
    assert_eq!(fingerprint.os_id.as_str(), "auraos");
    assert!(
        fingerprint.version.starts_with(b"1.0 (core)"),
        "version field populated"
    );
    assert_eq!(fingerprint.version_codename.as_str(), "core");

    // meta.dual_clock: finalize stamps both the monotonic freshness clock and
    // the absolute wallclock, and seals the archive exactly once.
    let mut state = FixedCollectorState::default();
    let clock = FixedClock {
        monotonic_ns: 42_000_000,
        wallclock_ns: 1_700_000_000_000_000_000,
    };
    let mut finalizer = SystemFinalizer::new(clock);
    finalizer.finalize(&mut state).expect("finalize");
    assert_eq!(state.archive.meta.timestamp_ns, 42_000_000, "monotonic");
    assert_eq!(
        state.archive.meta.wallclock_ns, 1_700_000_000_000_000_000,
        "wallclock"
    );
    assert!(
        state.archive.capabilities & CAP_META_WALLCLOCK != 0,
        "wallclock capability published"
    );
    assert_eq!(
        state.archive.checksum,
        state.archive.calculate_checksum(),
        "sealed once"
    );
}

// -----------------------------------------------------------------------
// AUD-013 — no-follow secure modes; CRC32 is corruption detection only
// -----------------------------------------------------------------------

#[test]
#[cfg(target_os = "linux")]
fn aud013_security_modes_and_crc_scope() {
    check_row(row(12));

    use std::os::unix::fs::PermissionsExt;

    // security.no_follow_modes: symlinks at the state or lock path are
    // rejected as Security and the target is never touched; fresh state and
    // lock leaves are created 0600.
    let target_dir = tempfile::tempdir().expect("target dir");
    let target = target_dir.path().join("victim");
    fs::write(&target, b"sentinel").expect("seed target");

    let symlink_dir = tempfile::tempdir().expect("symlink dir");
    fs::set_permissions(symlink_dir.path(), fs::Permissions::from_mode(0o700))
        .expect("private dir");
    let link = symlink_dir.path().join("state.dat");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    match aura_daemon::state::ShmHandle::new(&link) {
        Err(AuraError::Security(_)) => {}
        other => panic!("state symlink must be a Security rejection: {other:?}"),
    }
    assert_eq!(fs::read(&target).expect("target read"), b"sentinel");

    let locklink_dir = tempfile::tempdir().expect("locklink dir");
    fs::set_permissions(locklink_dir.path(), fs::Permissions::from_mode(0o700))
        .expect("private dir");
    std::os::unix::fs::symlink(&target, locklink_dir.path().join("state.dat.lock"))
        .expect("lock symlink");
    match aura_daemon::state::ShmHandle::new(&locklink_dir.path().join("state.dat")) {
        Err(AuraError::Security(_)) => {}
        other => panic!("lock symlink must be a Security rejection: {other:?}"),
    }
    assert_eq!(fs::read(&target).expect("target read"), b"sentinel");

    let fresh_dir = tempfile::tempdir().expect("fresh dir");
    fs::set_permissions(fresh_dir.path(), fs::Permissions::from_mode(0o700)).expect("private dir");
    let state_path = fresh_dir.path().join("state.dat");
    let handle = aura_daemon::state::ShmHandle::new(&state_path).expect("fresh state");
    let state_mode = fs::metadata(&state_path)
        .expect("state")
        .permissions()
        .mode()
        & 0o777;
    let lock_mode = fs::metadata(fresh_dir.path().join("state.dat.lock"))
        .expect("lock")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(state_mode, 0o600, "state leaf 0600");
    assert_eq!(lock_mode, 0o600, "lock leaf 0600");
    drop(handle);

    // abi.crc_corruption_only: flipping any data byte is detected by CRC32
    // (integrity), and the ADR records the rejected authentication premise.
    let mut archive = TelemetryArchive::zeroed();
    archive.version = aura_common::ARCHIVE_VERSION;
    archive.meta.uptime_secs = 9;
    let clean = archive.calculate_checksum();
    archive.meta.uptime_secs = 10;
    assert_ne!(
        archive.calculate_checksum(),
        clean,
        "corruption changes the checksum"
    );
    let adr = read_repo("docs/adr.md");
    assert!(
        adr.contains("ADR-005") && adr.contains("integrity only, never authentication"),
        "ADR-005 records the corruption-only CRC scope"
    );
}

// -----------------------------------------------------------------------
// AUD-014 — design documents are tracked source
// -----------------------------------------------------------------------

#[test]
fn aud014_docs_tracked_source_manifest() {
    check_row(row(13));

    // docs.tracked_source_manifest: the blanket markdown ignore is gone, the
    // private .omo ignore is retained, and every baseline document is tracked
    // by git (probed against the live index, not the filesystem).
    let gitignore = read_repo(".gitignore");
    assert!(
        !gitignore.lines().any(|line| line.trim() == "*.md"),
        "blanket *.md ignore removed"
    );
    assert!(
        gitignore.lines().any(|line| line.trim() == "/.omo/"),
        "private evidence ignore retained"
    );
    let tracked = [
        "README.md",
        "docs/adr.md",
        "docs/design-compliance-audit-2026-08-30.md",
        "docs/project.md",
        "docs/tech_blueprint.md",
    ];
    let output = std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch"])
        .args(tracked)
        .current_dir(repo_root())
        .output()
        .expect("git ls-files");
    assert!(
        output.status.success(),
        "all baseline documents tracked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for document in tracked {
        assert!(
            !read_repo(document).is_empty(),
            "{document} is nonempty source"
        );
    }
}

// -----------------------------------------------------------------------
// AUD-015 — this matrix is the fresh post-gate coverage evidence
// -----------------------------------------------------------------------

#[test]
fn aud015_post_gate_fresh_evidence() {
    check_row(row(14));
    normative_binding();

    // evidence.post_gate_fresh: the matrix covers exactly AUD-001..=015 in
    // order with unique sorted assertion namespaces, and every registry ID it
    // references resolves to a file that exists in this checkout — a stale
    // historical PASS cannot satisfy any of these links.
    for (index, row) in MATRIX.iter().enumerate() {
        let expected = format!("AUD-{:03}", index + 1);
        assert_eq!(row.aud, expected, "matrix order/coverage drift");
        assert!(!row.assertions.is_empty(), "{} has assertions", row.aud);
        assert!(!row.evidence.is_empty(), "{} has evidence", row.aud);
        let mut sorted = row.assertions.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, row.assertions, "{} assertions not sorted", row.aud);
        let mut evidence_sorted = row.evidence.to_vec();
        evidence_sorted.sort_unstable();
        assert_eq!(
            evidence_sorted, row.evidence,
            "{} evidence not sorted",
            row.aud
        );
        for assertion in row.assertions {
            assert!(
                assertion.contains('.'),
                "assertion {assertion} is namespaced"
            );
        }
    }
    let namespaces: Vec<&str> = MATRIX
        .iter()
        .flat_map(|row| row.assertions.iter().copied())
        .collect();
    let mut unique = namespaces.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(namespaces.len(), unique.len(), "assertion IDs are unique");

    for (id, file) in EVIDENCE_FILES {
        assert!(
            repo_root().join(file).is_file(),
            "evidence suite {id} file missing: {file}"
        );
    }
    for (script, reason) in [
        ("scripts/verify-tip-gates.py", "tip gate"),
        ("scripts/check-rust-loc.py", "PLOC gate"),
    ] {
        assert!(
            repo_root().join(script).is_file(),
            "{reason} script missing: {script}"
        );
    }
}
