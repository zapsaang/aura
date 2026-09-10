pub(super) type MachPort = libc::c_uint;
pub(super) type KernReturn = libc::c_int;
pub(super) type ProcessorInfoArray = *mut libc::c_int;
pub(super) type MachMsgTypeNumber = libc::c_uint;

pub(super) const KERN_SUCCESS: KernReturn = 0;
pub(super) const KERN_FAILURE: KernReturn = 5;
pub(super) const PROCESSOR_CPU_LOAD_INFO: libc::c_int = 2;
pub(super) const HOST_VM_INFO64: libc::c_int = 4;
pub(super) const EINVAL: libc::c_int = 22;
/// Routing-sysctl MIB selectors for the NET_RT_IFLIST2 table dump.
pub(super) const CTL_NET: libc::c_int = 4;
pub(super) const PF_ROUTE: libc::c_int = 17;
pub(super) const NET_RT_IFLIST2: libc::c_int = 6;

extern "C" {
    pub(super) fn mach_host_self() -> MachPort;
    pub(super) fn mach_task_self() -> MachPort;
    pub(super) fn sysctl(
        mib: *const libc::c_int,
        mib_len: libc::c_uint,
        oldp: *mut libc::c_void,
        oldlenp: *mut libc::size_t,
        newp: *const libc::c_void,
        newlen: libc::size_t,
    ) -> libc::c_int;
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
