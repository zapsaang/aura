//! Process collector conformance tests (Todo 8 normative cells).

use aura_common::{
    ProcessStat, ProcessStats, TelemetryArchive, CAP_PROCESS_BLOCKED, CAP_PROCESS_RUNNING,
    CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY, CAP_PROCESS_TOTAL,
    PROCESS_TRUNCATED,
};
use aura_daemon::collectors::process::{mark_unavailable, ProcessAvailability};

#[cfg(target_os = "linux")]
use aura_common::{AuraError, AuraResult, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, MAX_CORES, MAX_TOP_N};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::process::linux::{collect, parse_proc_stat, ProcessScan};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::process::state::{
    ProcessBaseSnapshot, ProcessBaseline, PROCESS_BASELINE_CAPACITY,
};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::process::validate_page_size;
#[cfg(target_os = "linux")]
use aura_daemon::collectors::{
    CollectorScratch, CycleCollector, FixedCollectorState, ProviderOutcome, SystemCollector,
};
#[cfg(target_os = "linux")]
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
#[cfg(target_os = "linux")]
use aura_daemon::lifecycle::Finalizer;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod support;

fn assert_process_zero(process: &ProcessStats) {
    assert_eq!(process.total, 0);
    assert_eq!(process.running, 0);
    assert_eq!(process.blocked, 0);
    assert_eq!(process.sleeping, 0);
    assert_eq!(process.top_cpu_count, 0);
    assert_eq!(process.top_mem_count, 0);
    assert_eq!(process.flags, 0);
    assert_eq!(process._pad0, [0; 5]);
    assert!(process
        .top_cpu
        .iter()
        .chain(process.top_mem.iter())
        .all(|entry| entry.pid == 0
            && entry.cpu_usage == 0.0
            && entry.memory_bytes == 0
            && entry.comm.bytes == [0; 16]));
}

/// Builds a /proc/<pid>/stat line with fields at kernel positions:
/// pid 1, comm 2, state 3, utime 14, stime 15, starttime 22, rss 24.
#[cfg(target_os = "linux")]
fn stat_line(
    pid: u32,
    comm: &[u8],
    state: u8,
    utime: u64,
    stime: u64,
    starttime: u64,
    rss: i64,
) -> Vec<u8> {
    let mut line = Vec::new();
    line.extend_from_slice(pid.to_string().as_bytes());
    line.extend_from_slice(b" (");
    line.extend_from_slice(comm);
    line.extend_from_slice(b") ");
    line.push(state);
    line.extend_from_slice(
        format!(
            " 1 {pid} {pid} 0 -1 4194304 100 0 0 0 {utime} {stime} 0 0 20 0 1 0 {starttime} 200 {rss} 1 1 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0"
        )
        .as_bytes(),
    );
    line
}

#[cfg(target_os = "linux")]
fn write_pid(root: &std::path::Path, pid: u32, stat: &[u8]) {
    let dir = root.join(pid.to_string());
    std::fs::create_dir_all(&dir).expect("pid dir");
    std::fs::write(dir.join("stat"), stat).expect("stat");
}

#[cfg(target_os = "linux")]
fn run_collect(
    root: &std::path::Path,
    page_size: u64,
    online_cores: u64,
    delta_global: u64,
    baseline: &mut ProcessBaseline,
) -> (aura_common::ProcessStats, AuraResult<()>) {
    let mut out = TelemetryArchive::zeroed().process;
    let result = run_collect_into(
        root,
        page_size,
        online_cores,
        delta_global,
        baseline,
        &mut out,
    );
    (out, result)
}

#[cfg(target_os = "linux")]
fn run_collect_into(
    root: &std::path::Path,
    page_size: u64,
    online_cores: u64,
    delta_global: u64,
    baseline: &mut ProcessBaseline,
    out: &mut aura_common::ProcessStats,
) -> AuraResult<()> {
    let mut stat_buf = Vec::with_capacity(4096);
    let mut path_buf = Vec::with_capacity(4096);
    {
        let mut scan = ProcessScan {
            proc_root: root.as_os_str().as_bytes(),
            page_size,
            online_cores,
            delta_global_ticks: delta_global,
            stat_buf: &mut stat_buf,
            path_buf: &mut path_buf,
        };
        collect(&mut scan, baseline, out)
    }
}

// ---------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_stat_extracts_kernel_field_positions() {
    let fixture = stat_line(12345, b"my process", b'R', 100, 20, 4242, 256);
    let parsed = parse_proc_stat(&fixture).expect("must parse");
    assert_eq!(parsed.pid, 12345);
    assert_eq!(parsed.comm.as_str(), "my process");
    assert_eq!(parsed.state, b'R');
    assert_eq!(parsed.utime, 100);
    assert_eq!(parsed.stime, 20);
    assert_eq!(parsed.starttime, 4242);
    assert_eq!(parsed.rss_pages, 256);
}

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_stat_uses_final_close_paren_for_comm() {
    let fixture = stat_line(77, b"my (odd) name", b'S', 11, 22, 333, 44);
    let parsed = parse_proc_stat(&fixture).expect("must parse");
    assert_eq!(parsed.comm.as_str(), "my (odd) name");
    assert_eq!(parsed.state, b'S');
    assert_eq!(parsed.utime, 11);
    assert_eq!(parsed.stime, 22);
    assert_eq!(parsed.starttime, 333);
    assert_eq!(parsed.rss_pages, 44);
}

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_stat_preserves_negative_rss() {
    let fixture = stat_line(5, b"neg", b'R', 1, 1, 1, -22);
    let parsed = parse_proc_stat(&fixture).expect("must parse");
    assert_eq!(parsed.rss_pages, -22i64);
}

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_stat_rejects_malformed() {
    assert!(parse_proc_stat(b"not a stat line").is_none());
    assert!(parse_proc_stat(b"12345 (no closing paren R 1 2 3").is_none());
    assert!(parse_proc_stat(b"1 (x)").is_none());
    assert!(
        parse_proc_stat(b"0 (zero) R 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 0 1 0 1 1 0 0 0 0 1 1").is_none()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_stat_requires_space_after_state() {
    let mut fixture = stat_line(7, b"framing", b'R', 11, 22, 33, 44);
    let close = fixture
        .iter()
        .rposition(|byte| *byte == b')')
        .expect("close parenthesis");
    fixture[close + 3] = b'X';

    assert!(
        parse_proc_stat(&fixture).is_none(),
        "`) RX1` framing must not shift later fields"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn parse_proc_stat_rejects_invalid_comm_bytes() {
    let control = stat_line(1, b"bad\x01name", b'R', 1, 1, 1, 1);
    assert!(parse_proc_stat(&control).is_none(), "control comm rejected");
    let non_utf8 = stat_line(1, b"bad\xffname", b'R', 1, 1, 1, 1);
    assert!(
        parse_proc_stat(&non_utf8).is_none(),
        "non-UTF-8 comm rejected"
    );
    let newline = stat_line(1, b"bad\nname", b'R', 1, 1, 1, 1);
    assert!(parse_proc_stat(&newline).is_none(), "newline comm rejected");
    let empty = stat_line(1, b"", b'R', 1, 1, 1, 1);
    assert!(parse_proc_stat(&empty).is_none(), "empty comm rejected");
}

// ---------------------------------------------------------------------
// Page size (cached at collector init; bad values are Fatal)
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn validate_page_size_maps_bad_values_to_fatal() {
    for bad in [-1i64, 0, i64::MIN, -4096] {
        let err = validate_page_size(bad).expect_err("must reject");
        assert!(
            matches!(err, AuraError::Fatal(_)),
            "page size {bad} must be Fatal"
        );
    }
    assert_eq!(validate_page_size(4096).expect("ok"), 4096);
    assert_eq!(validate_page_size(16384).expect("ok"), 16384);
}

#[cfg(target_os = "linux")]
#[test]
fn collect_with_zero_page_size_is_fatal() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(dir.path(), 7, &stat_line(7, b"job", b'R', 1, 1, 7, 10));
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 0, 1, 100, &mut baseline);
    assert!(matches!(result, Err(AuraError::Fatal(_))));
    assert_eq!(out.total, 0, "fatal path publishes nothing");
    assert!(baseline.get(&(7, 7)).is_none(), "fatal commits no baseline");
}

// ---------------------------------------------------------------------
// Enumeration failure: Fatal, commits no generation/reclamation
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn collect_enumeration_failure_is_fatal_and_commits_nothing() {
    let mut baseline = ProcessBaseline::default();
    let key = (55u32, 66u64);
    let generation = baseline.insert(key, ProcessBaseSnapshot { utime: 1, stime: 2 });
    let before = baseline.generation();
    let (out, result) = run_collect(
        std::path::Path::new("/nonexistent-aura-proc"),
        4096,
        1,
        100,
        &mut baseline,
    );
    assert!(matches!(result, Err(AuraError::Fatal(_))));
    assert_eq!(out.total, 0);
    let slot = baseline.get(&key).expect("entry survives fatal");
    assert_eq!(slot.generation, generation, "no reclamation on fatal");
    assert_eq!(
        baseline.generation(),
        before,
        "no generation commit on fatal"
    );
}

// ---------------------------------------------------------------------
// State counting (every state, exact mapping)
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn collect_counts_every_state_exactly() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let cases = [
        (100u32, b'R'),
        (101, b'D'),
        (102, b'S'),
        (103, b'I'),
        (104, b'T'),
        (105, b't'),
        (106, b'Z'),
        (107, b'X'),
    ];
    for (pid, state) in cases {
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"p", state, 1, 1, pid as u64, 10),
        );
    }
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    result.expect("collect");
    assert_eq!(out.total, 8);
    assert_eq!(out.running, 1, "R");
    assert_eq!(out.blocked, 1, "D");
    assert_eq!(out.sleeping, 4, "S|I|T|t");
}

// ---------------------------------------------------------------------
// Skip + truncate semantics
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn collect_skips_raced_or_malformed_and_sets_truncation() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(dir.path(), 1, b"not a stat line");
    for pid in [2u32, 3] {
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"ok", b'R', 1, 1, pid as u64, 10),
        );
    }
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    result.expect("collect succeeds despite per-record loss");
    assert_eq!(out.total, 2, "malformed record is skipped, not counted");
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
    assert_eq!(out.flags & !PROCESS_TRUNCATED, 0, "only bit0 may be set");
}

#[cfg(target_os = "linux")]
#[test]
fn collect_skips_rss_overflow_and_sets_truncation() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(
        dir.path(),
        10,
        &stat_line(10, b"over", b'R', 1, 1, 10, i64::MAX),
    );
    write_pid(dir.path(), 11, &stat_line(11, b"fine", b'R', 1, 1, 11, 10));
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    result.expect("collect");
    assert_eq!(out.total, 1, "overflow record skipped entirely");
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
}

#[cfg(target_os = "linux")]
#[test]
fn collect_skips_negative_rss_and_sets_truncation() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(dir.path(), 20, &stat_line(20, b"neg", b'R', 1, 1, 20, -5));
    write_pid(dir.path(), 21, &stat_line(21, b"fine", b'R', 1, 1, 21, 10));
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    result.expect("collect");
    assert_eq!(out.total, 1);
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
}

#[cfg(target_os = "linux")]
#[test]
fn collect_skips_invalid_comm_and_sets_truncation() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(
        dir.path(),
        30,
        &stat_line(30, b"bad\x01comm", b'R', 1, 1, 30, 10),
    );
    write_pid(dir.path(), 31, &stat_line(31, b"", b'R', 1, 1, 31, 10));
    write_pid(dir.path(), 32, &stat_line(32, b"fine", b'R', 1, 1, 32, 10));
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    result.expect("collect");
    assert_eq!(out.total, 1, "control and empty comm records skipped");
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
}

#[cfg(target_os = "linux")]
#[test]
fn collect_clean_cycle_has_zero_flags() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(dir.path(), 40, &stat_line(40, b"fine", b'R', 1, 1, 40, 10));
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    result.expect("collect");
    assert_eq!(out.flags, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn truncated_cycle_followed_by_clean_cycle_clears_flag_on_same_output() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(dir.path(), 41, b"malformed");
    let mut baseline = ProcessBaseline::default();
    let mut out = TelemetryArchive::zeroed().process;

    run_collect_into(dir.path(), 4096, 1, 100, &mut baseline, &mut out).expect("truncated cycle");
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);

    std::fs::remove_dir_all(dir.path().join("41")).expect("remove malformed pid");
    write_pid(dir.path(), 42, &stat_line(42, b"clean", b'R', 1, 1, 42, 10));
    run_collect_into(dir.path(), 4096, 1, 100, &mut baseline, &mut out).expect("clean cycle");

    assert_eq!(out.flags, 0, "clean cycle recomputes process flags");
    assert_eq!(out.total, 1);
    assert_eq!(out.top_mem_count, 1);
}

// ---------------------------------------------------------------------
// Interval CPU deltas, formula, seeding
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn first_observation_seeds_baseline_without_cpu_candidate() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(dir.path(), 7, &stat_line(7, b"first", b'R', 100, 20, 7, 10));
    let mut baseline = ProcessBaseline::default();
    let (out, result) = run_collect(dir.path(), 4096, 2, 240, &mut baseline);
    result.expect("collect");
    assert_eq!(out.top_cpu_count, 0, "first observation has no valid delta");
    assert!(baseline.get(&(7, 7)).is_some(), "identity seeded");
}

#[cfg(target_os = "linux")]
#[test]
fn interval_cpu_uses_plan_formula() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let pid_dir = dir.path().join("8");
    std::fs::create_dir(&pid_dir).expect("pid dir");

    let mut baseline = ProcessBaseline::default();
    std::fs::write(
        pid_dir.join("stat"),
        stat_line(8, b"two", b'R', 100, 20, 8, 10),
    )
    .expect("c1");
    let (_out1, r1) = run_collect(dir.path(), 4096, 2, 240, &mut baseline);
    r1.expect("cycle 1");

    std::fs::write(
        pid_dir.join("stat"),
        stat_line(8, b"two", b'R', 200, 40, 8, 10),
    )
    .expect("c2");
    let (out2, r2) = run_collect(dir.path(), 4096, 2, 240, &mut baseline);
    r2.expect("cycle 2");

    assert_eq!(out2.top_cpu_count, 1);
    let entry = out2.top_cpu[0];
    assert_eq!(entry.pid, 8);
    assert_eq!(entry.comm.as_str(), "two");
    // 100 * delta(120 ticks) * 2 cores / 240 global ticks = 100.0
    assert_eq!(entry.cpu_usage, 100.0);
}

#[cfg(target_os = "linux")]
#[test]
fn cpu_percent_can_exceed_100() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let pid_dir = dir.path().join("9");
    std::fs::create_dir(&pid_dir).expect("pid dir");

    let mut baseline = ProcessBaseline::default();
    std::fs::write(
        pid_dir.join("stat"),
        stat_line(9, b"hot", b'R', 0, 0, 9, 10),
    )
    .expect("c1");
    let (_o, r1) = run_collect(dir.path(), 4096, 4, 100, &mut baseline);
    r1.expect("cycle 1");

    std::fs::write(
        pid_dir.join("stat"),
        stat_line(9, b"hot", b'R', 50, 0, 9, 10),
    )
    .expect("c2");
    let (out2, r2) = run_collect(dir.path(), 4096, 4, 100, &mut baseline);
    r2.expect("cycle 2");

    assert_eq!(out2.top_cpu_count, 1);
    // 100 * 50 * 4 / 100 = 200.0
    assert_eq!(out2.top_cpu[0].cpu_usage, 200.0, ">100% CPU is correct");
}

#[cfg(target_os = "linux")]
#[test]
fn counter_decrease_reseeds_without_candidate() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let pid_dir = dir.path().join("12");
    std::fs::create_dir(&pid_dir).expect("pid dir");

    let mut baseline = ProcessBaseline::default();
    std::fs::write(
        pid_dir.join("stat"),
        stat_line(12, b"dec", b'R', 200, 0, 12, 10),
    )
    .expect("c1");
    let (_o, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 1");

    std::fs::write(
        pid_dir.join("stat"),
        stat_line(12, b"dec", b'R', 100, 0, 12, 10),
    )
    .expect("c2");
    let (out2, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 2");
    assert_eq!(
        out2.top_cpu_count, 0,
        "counter decrease is not a valid delta"
    );

    std::fs::write(
        pid_dir.join("stat"),
        stat_line(12, b"dec", b'R', 150, 0, 12, 10),
    )
    .expect("c3");
    let (out3, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 3");
    assert_eq!(out3.top_cpu_count, 1, "reseeds from the decreased value");
    assert_eq!(out3.top_cpu[0].cpu_usage, 50.0);
}

#[cfg(target_os = "linux")]
#[test]
fn pid_reuse_seeds_fresh_identity() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let pid_dir = dir.path().join("13");
    std::fs::create_dir(&pid_dir).expect("pid dir");

    let mut baseline = ProcessBaseline::default();
    std::fs::write(
        pid_dir.join("stat"),
        stat_line(13, b"re", b'R', 100, 0, 1000, 10),
    )
    .expect("c1");
    let (_o, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 1");

    std::fs::write(
        pid_dir.join("stat"),
        stat_line(13, b"re", b'R', 50, 0, 2000, 10),
    )
    .expect("c2");
    let (out2, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 2");
    assert_eq!(
        out2.top_cpu_count, 0,
        "same pid with new starttime is a fresh identity"
    );

    std::fs::write(
        pid_dir.join("stat"),
        stat_line(13, b"re", b'R', 90, 0, 2000, 10),
    )
    .expect("c3");
    let (out3, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 3");
    assert_eq!(out3.top_cpu_count, 1);
    assert_eq!(out3.top_cpu[0].cpu_usage, 40.0);
}

// ---------------------------------------------------------------------
// Reclamation: sweep + deterministic rehash
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn disappearance_reclaims_entry_after_sweep_and_rehash() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let pid_dir = dir.path().join("21");
    std::fs::create_dir(&pid_dir).expect("pid dir");
    std::fs::write(
        pid_dir.join("stat"),
        stat_line(21, b"gone", b'R', 1, 1, 21, 10),
    )
    .expect("c1");

    let mut baseline = ProcessBaseline::default();
    let (out1, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 1");
    assert_eq!(out1.total, 1);
    assert!(baseline.get(&(21, 21)).is_some());

    std::fs::remove_dir_all(&pid_dir).expect("remove");
    let (out2, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 2");
    assert_eq!(out2.total, 0);
    assert_eq!(out2.top_cpu_count, 0);
    assert_eq!(out2.top_mem_count, 0);
    assert!(
        baseline.get(&(21, 21)).is_none(),
        "unobserved identity reclaimed after sweep"
    );

    write_pid(dir.path(), 22, &stat_line(22, b"new", b'R', 5, 5, 22, 10));
    let (out3, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("cycle 3");
    assert_eq!(out3.total, 1, "rehashed table accepts new identities");
    assert!(baseline.get(&(22, 22)).is_some());
}

#[cfg(target_os = "linux")]
#[test]
fn baseline_survives_repeated_churn_cycles() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let mut baseline = ProcessBaseline::default();
    let mut global = 1000u64;
    for cycle in 0..12u32 {
        let active: &[u32] = if cycle % 2 == 0 {
            &[100, 101, 102, 103]
        } else {
            &[102, 103, 104, 105]
        };
        for pid in active {
            write_pid(
                dir.path(),
                *pid,
                &stat_line(*pid, b"churn", b'R', cycle as u64, 0, *pid as u64, 10),
            );
        }
        global += 100;
        let (out, r) = run_collect(dir.path(), 4096, 1, global, &mut baseline);
        r.expect("collect");
        assert_eq!(out.total, 4, "cycle {cycle}");
        for pid in [100u32, 101, 104, 105] {
            let present = active.contains(&pid);
            assert_eq!(
                baseline.get(&(pid, pid as u64)).is_some(),
                present,
                "pid {pid} cycle {cycle}"
            );
        }
        for pid in [100u32, 101, 102, 103, 104, 105] {
            let d = dir.path().join(pid.to_string());
            if d.exists() {
                std::fs::remove_dir_all(d).expect("reset");
            }
        }
    }
}

// ---------------------------------------------------------------------
// Capacity: 4097 identities, overflow never evicts active identity
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn scan_with_4097_identities_counts_all_and_truncates() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let total_pids = (PROCESS_BASELINE_CAPACITY + 1) as u32;
    for pid in 1..=total_pids {
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"cap", b'R', 0, 0, pid as u64, 10),
        );
    }
    let mut baseline = ProcessBaseline::default();
    let (out, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("collect");
    assert_eq!(out.total, total_pids, "every parsed record counts");
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
    assert_eq!(out.top_mem_count as usize, MAX_TOP_N);
    assert_eq!(
        out.top_cpu_count, 0,
        "first observation has no cpu candidates"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn baseline_overflow_never_evicts_active_identity_and_memory_still_ranks() {
    use tempfile::TempDir;
    let mut baseline = ProcessBaseline::default();
    for pid in 1..=(PROCESS_BASELINE_CAPACITY as u32) {
        baseline.insert((pid, 7), ProcessBaseSnapshot { utime: 0, stime: 0 });
    }
    let dir = TempDir::new().expect("tmpdir");
    // pid 100 holds an existing (active) identity; pid 5000 is the 4097th distinct key.
    write_pid(
        dir.path(),
        100,
        &stat_line(100, b"act", b'R', 100, 0, 7, 100),
    );
    write_pid(
        dir.path(),
        5000,
        &stat_line(5000, b"over", b'R', 100, 0, 9, 200),
    );

    let (out, r) = run_collect(dir.path(), 4096, 1, 1000, &mut baseline);
    r.expect("collect");
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
    assert!(baseline.get(&(100, 7)).is_some(), "active identity kept");
    assert!(
        baseline.get(&(5000, 9)).is_none(),
        "overflow identity not tracked"
    );
    assert_eq!(
        out.top_mem_count, 2,
        "overflow identity still ranks by memory"
    );
    assert_eq!(out.top_mem[0].pid, 5000);
    assert_eq!(out.top_mem[0].memory_bytes, 200 * 4096);
    assert_eq!(out.top_mem[1].pid, 100);
    assert_eq!(out.top_cpu_count, 1, "active identity keeps valid delta");
    assert_eq!(out.top_cpu[0].pid, 100);
}

#[cfg(target_os = "linux")]
#[test]
fn zero_rss_records_rank_and_participate_in_truncation() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    for pid in 1..=6u32 {
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"zero-rss", b'S', 0, 0, u64::from(pid), 0),
        );
    }
    let mut baseline = ProcessBaseline::default();

    let (out, result) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);

    result.expect("collect zero RSS records");
    assert_eq!(out.total, 6);
    assert_eq!(out.top_mem_count as usize, MAX_TOP_N);
    assert_eq!(out.flags & PROCESS_TRUNCATED, PROCESS_TRUNCATED);
    let pids: Vec<u32> = out.top_mem.iter().map(|entry| entry.pid).collect();
    assert_eq!(pids, [1, 2, 3, 4, 5]);
    assert!(out.top_mem.iter().all(|entry| entry.memory_bytes == 0));
}

// ---------------------------------------------------------------------
// Ordering: metric-desc, PID-asc
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn top_arrays_order_metric_desc_then_pid_asc() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    let deltas = [
        (10u32, 300u64),
        (20, 100),
        (30, 300),
        (40, 300),
        (50, 50),
        (60, 300),
        (70, 300),
    ];
    let rss = [
        (10u32, 100i64),
        (20, 900),
        (30, 900),
        (40, 10),
        (50, 10),
        (60, 10),
        (70, 10),
    ];

    let mut baseline = ProcessBaseline::default();
    for (pid, _) in deltas {
        let rss_pages = rss.iter().find(|(p, _)| *p == pid).unwrap().1;
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"ord", b'R', 0, 0, pid as u64, rss_pages),
        );
    }
    let (_o, r) = run_collect(dir.path(), 4096, 1, 1000, &mut baseline);
    r.expect("cycle 1");

    for (pid, delta) in deltas {
        let rss_pages = rss.iter().find(|(p, _)| *p == pid).unwrap().1;
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"ord", b'R', delta, 0, pid as u64, rss_pages),
        );
    }
    let (out, r) = run_collect(dir.path(), 4096, 1, 1000, &mut baseline);
    r.expect("cycle 2");

    assert_eq!(out.top_cpu_count as usize, MAX_TOP_N);
    assert_eq!(
        out.flags & PROCESS_TRUNCATED,
        PROCESS_TRUNCATED,
        "7 candidates > 5"
    );
    let cpu_pids: Vec<u32> = out.top_cpu[..5].iter().map(|e| e.pid).collect();
    assert_eq!(
        cpu_pids,
        [10, 30, 40, 60, 70],
        "delta 300 group, pid ascending"
    );
    for entry in &out.top_cpu[..5] {
        assert_eq!(entry.cpu_usage, 30.0);
    }
    let mem_pids: Vec<u32> = out.top_mem[..5].iter().map(|e| e.pid).collect();
    assert_eq!(mem_pids, [20, 30, 10, 40, 50], "rss desc, pid asc on ties");
    assert_eq!(out.top_mem[0].memory_bytes, 900 * 4096);
}

// ---------------------------------------------------------------------
// >128 cores: finalize clears Process top-CPU capability and arrays
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
struct FixedClock;

#[cfg(target_os = "linux")]
impl Clock for FixedClock {
    fn sample(&mut self) -> AuraResult<ClockSample> {
        Ok(ClockSample {
            monotonic_ns: 1_000_000_000,
            wallclock_ns: 1_700_000_000_000_000_000,
        })
    }
}

#[cfg(target_os = "linux")]
#[test]
fn production_cpu_path_detects_129_cores_and_clears_unrepresentable_arrays() {
    use support::system_sources::DeterministicSources;
    let mut sources = DeterministicSources::default();
    sources.over_capacity_cpu_fixture = true;
    let mut collector = SystemCollector::with_sources(sources);
    let mut state = FixedCollectorState::default();
    let mut scratch = CollectorScratch::default();

    let outcome = collector.collect(&mut state, &mut scratch);
    assert!(matches!(outcome, ProviderOutcome::Available(())));
    assert_eq!(state.archive.cpu.core_count as usize, MAX_CORES);

    let mut finalizer = SystemFinalizer::new(FixedClock);
    finalizer.finalize(&mut state).expect("finalize");

    let caps = state.archive.capabilities;
    assert_eq!(caps & CAP_PROCESS_TOP_CPU, 0, "top-CPU capability cleared");
    assert_eq!(caps & CAP_CPU_PER_CORE, 0, "per-core capability cleared");
    assert_ne!(caps & CAP_CPU_GLOBAL, 0, "cpu-global remains available");
    assert_eq!(
        state.archive.cpu.core_count as usize, MAX_CORES,
        "represented core count remains 128"
    );
    assert!(
        state.archive.cpu.cores.iter().all(|core| {
            core.user_ticks == 0
                && core.system_ticks == 0
                && core.idle_ticks == 0
                && core.total_ticks == 0
                && core.usage_percent == 0.0
        }),
        "per-core array zeroed"
    );
    assert_eq!(state.archive.process.top_cpu_count, 0);
    assert!(
        state
            .archive
            .process
            .top_cpu
            .iter()
            .all(|e| e.pid == 0 && e.cpu_usage == 0.0),
        "top_cpu array zeroed"
    );
    assert_ne!(caps & CAP_PROCESS_TOTAL, 0, "other process caps retained");
    assert_eq!(state.archive.process.total, 42, "counts not clamped");
    aura_common::validate_archive(&state.archive).expect("archive contract holds");
}

// ---------------------------------------------------------------------
// Wiring: sources publish process capabilities into the archive
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn system_collector_wires_process_into_archive_and_capabilities() {
    use support::system_sources::DeterministicSources;
    let mut collector = SystemCollector::with_sources(DeterministicSources::default());
    let mut state = FixedCollectorState::default();
    let mut scratch = CollectorScratch::default();
    let outcome = collector.collect(&mut state, &mut scratch);
    assert!(matches!(outcome, ProviderOutcome::Available(())));
    let process_caps = CAP_PROCESS_TOTAL
        | CAP_PROCESS_RUNNING
        | CAP_PROCESS_BLOCKED
        | CAP_PROCESS_SLEEPING
        | CAP_PROCESS_TOP_CPU
        | CAP_PROCESS_TOP_MEMORY;
    assert_eq!(
        state.archive.capabilities & process_caps,
        process_caps,
        "all five process capability bits set by wired collection"
    );
    assert_eq!(state.archive.process.total, 42);
    assert_eq!(state.archive.process.running, 40);
}

// ---------------------------------------------------------------------
// Zero allocation after warm-up (50 measured cycles)
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    pub struct CountingAllocator;

    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    static ACTIVE: AtomicBool = AtomicBool::new(false);

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub fn start() {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
    }

    pub fn finish() -> usize {
        ACTIVE.store(false, Ordering::Relaxed);
        ALLOCATIONS.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "linux")]
#[global_allocator]
static PROCESS_TEST_ALLOCATOR: alloc_probe::CountingAllocator = alloc_probe::CountingAllocator;

#[cfg(target_os = "linux")]
#[test]
fn fixture_path_allocates_zero_after_warmup() {
    const CHILD_MARKER: &str = "AURA_PROCESS_ALLOC_PROBE_CHILD";
    if std::env::var_os(CHILD_MARKER).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("fixture_path_allocates_zero_after_warmup")
            .arg("--test-threads=1")
            .env(CHILD_MARKER, "1")
            .output()
            .expect("run isolated allocation probe");
        assert!(
            output.status.success(),
            "isolated allocation probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    for pid in 1..=6u32 {
        write_pid(
            dir.path(),
            pid,
            &stat_line(pid, b"probe", b'R', 100, 20, pid as u64, 50),
        );
    }
    let root = dir.path().as_os_str().as_bytes().to_vec();
    let mut baseline = ProcessBaseline::default();
    let mut stat_buf = Vec::with_capacity(4096);
    let mut path_buf = Vec::with_capacity(4096);
    let mut out = TelemetryArchive::zeroed().process;

    for _ in 0..2 {
        let mut scan = ProcessScan {
            proc_root: &root,
            page_size: 4096,
            online_cores: 2,
            delta_global_ticks: 1000,
            stat_buf: &mut stat_buf,
            path_buf: &mut path_buf,
        };
        collect(&mut scan, &mut baseline, &mut out).expect("warm-up");
    }

    alloc_probe::start();
    for _ in 0..50 {
        let mut scan = ProcessScan {
            proc_root: &root,
            page_size: 4096,
            online_cores: 2,
            delta_global_ticks: 1000,
            stat_buf: &mut stat_buf,
            path_buf: &mut path_buf,
        };
        collect(&mut scan, &mut baseline, &mut out).expect("measured cycle");
    }
    let calls = alloc_probe::finish();
    assert_eq!(calls, 0, "fixture path must allocate zero after warm-up");
    assert_eq!(out.total, 6);
}

// ---------------------------------------------------------------------
// FixedString16 ABI semantics: truncation at 16 bytes, no sanitization
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn valid_long_comm_truncates_at_16_like_network_names() {
    use tempfile::TempDir;
    let dir = TempDir::new().expect("tmpdir");
    write_pid(
        dir.path(),
        60,
        &stat_line(60, b"abcdefghijklmnopqr", b'R', 1, 1, 60, 10),
    );
    let mut baseline = ProcessBaseline::default();
    let (out, r) = run_collect(dir.path(), 4096, 1, 100, &mut baseline);
    r.expect("collect");
    assert_eq!(out.total, 1, "long valid comm is kept");
    assert_eq!(out.top_mem_count, 1);
    assert_eq!(&out.top_mem[0].comm.bytes, b"abcdefghijklmnop");
}

// ---------------------------------------------------------------------
// Forbidden symbols: no private/undocumented process enumeration
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn production_sources_contain_no_forbidden_process_symbols() {
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "proc_listallpids",
        "proc_pidinfo",
        "proc_name",
        "PROC_PIDTASKINFO",
        "PROC_PIDTBSDINFO",
        "KERN_PROC",
        "libproc",
    ];
    let mut stack = vec![src_root];
    let mut scanned = 0usize;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let content = std::fs::read(&path).expect("read source");
                let text = String::from_utf8_lossy(&content);
                for symbol in forbidden {
                    assert!(
                        !text.contains(symbol),
                        "forbidden symbol {symbol} in {}",
                        path.display()
                    );
                }
                scanned += 1;
            }
        }
    }
    assert!(scanned > 10, "denylist scan must cover production sources");
}

#[cfg(target_os = "linux")]
#[test]
fn linked_daemon_contains_no_forbidden_process_symbols() {
    let binary = std::fs::read(env!("CARGO_BIN_EXE_aura-daemon")).expect("read linked daemon");
    for symbol in ["proc_listallpids", "proc_pidinfo", "proc_name", "KERN_PROC"] {
        assert!(
            !binary
                .windows(symbol.len())
                .any(|window| window == symbol.as_bytes()),
            "linked daemon contains forbidden symbol {symbol}"
        );
    }
}

// ---------------------------------------------------------------------
// macOS: Unavailable with zeroed Process
// ---------------------------------------------------------------------

#[test]
fn unavailable_process_contract_clears_capabilities_and_zeroes_shape() {
    let mut process = TelemetryArchive::zeroed().process;
    process.total = 9;
    process.running = 8;
    process.blocked = 7;
    process.sleeping = 6;
    process.top_cpu_count = 1;
    process.top_mem_count = 1;
    process.flags = PROCESS_TRUNCATED;
    process.top_cpu[0].pid = 11;
    process.top_mem[0].pid = 12;

    mark_unavailable(&mut process);
    let availability = ProcessAvailability::unavailable();

    assert!(!availability.running);
    assert!(!availability.total);
    assert_eq!(availability.capability_mask(), 0);
    assert_process_zero(&process);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_platform_process_provider_returns_unavailable_and_zeroes_shape() {
    use aura_daemon::collectors::{CollectorSources, PlatformSources};
    let mut sources = PlatformSources;
    let mut state = FixedCollectorState::default();
    let mut scratch = CollectorScratch::default();
    state.archive.process.total = 9;
    state.archive.process.top_cpu_count = 1;
    state.archive.process.top_cpu[0].pid = 7;

    let availability = sources
        .collect_process(&mut state, &mut scratch)
        .expect("macOS process provider");

    assert_eq!(availability, ProcessAvailability::unavailable());
    assert_process_zero(&state.archive.process);
}

// ---------------------------------------------------------------------
// Cross-platform capability/flag contracts
// ---------------------------------------------------------------------

#[test]
fn cap_process_bits_cover_required_capabilities() {
    let caps = CAP_PROCESS_TOTAL
        | CAP_PROCESS_RUNNING
        | CAP_PROCESS_BLOCKED
        | CAP_PROCESS_SLEEPING
        | CAP_PROCESS_TOP_CPU
        | CAP_PROCESS_TOP_MEMORY;
    assert_ne!(caps & CAP_PROCESS_TOTAL, 0);
    assert_ne!(caps & CAP_PROCESS_TOP_CPU, 0);
    assert_ne!(caps & CAP_PROCESS_TOP_MEMORY, 0);
}

#[test]
fn process_truncated_flag_is_distinct_bit() {
    assert_eq!(PROCESS_TRUNCATED, 1);
}

#[allow(dead_code)]
fn _ensure_linked() {
    let _: TelemetryArchive = TelemetryArchive::zeroed();
    let _: ProcessStat = ProcessStat::new();
}
