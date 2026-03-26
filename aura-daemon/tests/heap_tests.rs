use aura_common::{FixedString16, ProcessStat, MAX_TOP_N};
use aura_daemon::collectors::heap::{HeapEntry, MinHeap5};

fn make_process(pid: u32, cpu: f32, mem: u64) -> ProcessStat {
    ProcessStat {
        pid,
        cpu_usage: cpu,
        memory_bytes: mem,
        comm: FixedString16::new(),
    }
}

#[test]
fn min_heap5_insert_and_extract() {
    let mut heap = MinHeap5::new();
    heap.push(HeapEntry::new(100, make_process(1, 10.0, 100)));
    heap.push(HeapEntry::new(50, make_process(2, 5.0, 50)));
    heap.push(HeapEntry::new(200, make_process(3, 20.0, 200)));
    let result = heap.as_desc_array();
    assert_eq!(result[0].pid, 3);
    assert_eq!(result[1].pid, 1);
    assert_eq!(result[2].pid, 2);
}

#[test]
fn min_heap5_maintains_max_5() {
    let mut heap = MinHeap5::new();
    for i in 0..10u32 {
        heap.push(HeapEntry::new(
            i as u64,
            make_process(i, i as f32, i as u64),
        ));
    }
    let result = heap.as_desc_array();
    assert_eq!(result[0].pid, 9);
    assert_eq!(result[1].pid, 8);
    assert_eq!(result[2].pid, 7);
    assert_eq!(result[3].pid, 6);
    assert_eq!(result[4].pid, 5);
}

#[test]
fn min_heap5_empty_returns_zeroed() {
    let heap = MinHeap5::new();
    let result = heap.as_desc_array();
    for stat in &result {
        assert_eq!(stat.pid, 0);
        assert_eq!(stat.memory_bytes, 0);
    }
}

#[test]
fn min_heap5_exact_5_elements() {
    let mut heap = MinHeap5::new();
    for i in 0..MAX_TOP_N as u32 {
        heap.push(HeapEntry::new(
            (i + 1) as u64,
            make_process(i + 1, (i + 1) as f32, (i + 1) as u64),
        ));
    }
    let result = heap.as_desc_array();
    assert_eq!(result[0].pid, 5);
    assert_eq!(result[4].pid, 1);
}

#[test]
fn min_heap5_duplicate_keys() {
    let mut heap = MinHeap5::new();
    heap.push(HeapEntry::new(100, make_process(1, 10.0, 100)));
    heap.push(HeapEntry::new(100, make_process(2, 10.0, 100)));
    heap.push(HeapEntry::new(100, make_process(3, 10.0, 100)));
    let result = heap.as_desc_array();
    let mut pids: Vec<u32> = result
        .iter()
        .filter(|s| s.pid != 0)
        .map(|s| s.pid)
        .collect();
    pids.sort();
    assert_eq!(pids, vec![1, 2, 3]);
}

#[test]
fn min_heap5_single_element() {
    let mut heap = MinHeap5::new();
    heap.push(HeapEntry::new(42, make_process(7, 3.5, 999)));
    let result = heap.as_desc_array();
    assert_eq!(result[0].pid, 7);
    assert_eq!(result[0].memory_bytes, 999);
    assert_eq!(result[1].pid, 0);
}
