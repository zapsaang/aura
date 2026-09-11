use aura_common::{
    TelemetryArchive, CAP_PROCESS_BLOCKED, CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING,
    CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY, CAP_PROCESS_TOTAL,
};

use crate::args::ColorMode;

use super::color::{ARRAY_NA, ARRAY_NONE, NA};
use super::si::si;

fn count_line(out: &mut String, label: &str, owned: bool, value: u32) {
    out.push_str(label);
    if owned {
        out.push_str(&value.to_string());
    } else {
        out.push_str(NA);
    }
}

pub fn render(_color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let p = &t.process;
    let mut out = String::from("PROCESS\n");

    count_line(
        &mut out,
        "  total: ",
        caps & CAP_PROCESS_TOTAL != 0,
        p.total,
    );
    count_line(
        &mut out,
        "\n  running: ",
        caps & CAP_PROCESS_RUNNING != 0,
        p.running,
    );
    count_line(
        &mut out,
        "\n  blocked: ",
        caps & CAP_PROCESS_BLOCKED != 0,
        p.blocked,
    );
    count_line(
        &mut out,
        "\n  sleeping: ",
        caps & CAP_PROCESS_SLEEPING != 0,
        p.sleeping,
    );

    out.push_str("\n  top cpu:\n");
    if caps & CAP_PROCESS_TOP_CPU == 0 {
        out.push_str(ARRAY_NA);
    } else if p.top_cpu_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..p.top_cpu_count as usize)
            .map(|idx| {
                let e = &p.top_cpu[idx];
                format!(
                    "    {} {} {:.1}% {}",
                    e.pid,
                    e.comm.as_str(),
                    e.cpu_usage,
                    si(e.memory_bytes as f64)
                )
            })
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out.push_str("\n  top memory:\n");
    if caps & CAP_PROCESS_TOP_MEMORY == 0 {
        out.push_str(ARRAY_NA);
    } else if p.top_mem_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..p.top_mem_count as usize)
            .map(|idx| {
                let e = &p.top_mem[idx];
                format!(
                    "    {} {} {}",
                    e.pid,
                    e.comm.as_str(),
                    si(e.memory_bytes as f64)
                )
            })
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out
}
