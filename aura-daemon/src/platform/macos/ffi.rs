pub(super) type MachPort = libc::c_uint;
pub(super) type KernReturn = libc::c_int;
pub(super) type ProcessorInfoArray = *mut libc::c_int;
pub(super) type MachMsgTypeNumber = libc::c_uint;

pub(super) const KERN_SUCCESS: KernReturn = 0;
pub(super) const PROCESSOR_CPU_LOAD_INFO: libc::c_int = 2;
pub(super) const CPU_STATE_MAX: usize = 4;
pub(super) const HOST_VM_INFO64: libc::c_int = 4;

pub(super) const VM_STAT_FREE_COUNT: usize = 0;
pub(super) const VM_STAT_ACTIVE_COUNT: usize = 1;
pub(super) const VM_STAT_INACTIVE_COUNT: usize = 2;
pub(super) const VM_STAT_WIRE_COUNT: usize = 6;
pub(super) const VM_STAT_FAULTS: usize = 7;

extern "C" {
    pub(super) fn mach_host_self() -> MachPort;
    pub(super) fn mach_task_self() -> MachPort;
    pub(super) fn host_processor_info(
        host: MachPort,
        flavor: libc::c_int,
        out_processor_count: *mut libc::c_uint,
        out_processor_info: *mut ProcessorInfoArray,
        out_processor_info_count: *mut MachMsgTypeNumber,
    ) -> KernReturn;
    pub(super) fn host_statistics64(
        host: MachPort,
        flavor: libc::c_int,
        host_info: *mut libc::c_int,
        host_info_count: *mut MachMsgTypeNumber,
    ) -> KernReturn;
    pub(super) fn vm_deallocate(target_task: MachPort, address: usize, size: usize) -> KernReturn;
}
