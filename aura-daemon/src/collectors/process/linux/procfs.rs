//! Zero-allocation `/proc` directory and stat I/O.
//!
//! Directory walks and stat reads run entirely against caller-provided scratch
//! buffers. Raced, denied, or malformed stat reads skip the record, while
//! directory enumeration failures are Fatal.

use aura_common::{AuraError, AuraResult, MAX_PID};

use super::parse::parse_proc_stat;
use crate::collectors::process::state::ProcessProcStat;

pub const DIRENT_BUF_LEN: usize = 8192;
const DIRENT_HEADER_LEN: usize = 19;

pub trait ProcessDirectory {
    fn read(&mut self, buf: &mut [u8; DIRENT_BUF_LEN]) -> AuraResult<usize>;
}

/// Borrowed inputs for one `/proc` scan; every buffer is caller-owned so the
/// steady-state cycle performs zero heap allocation.
pub struct ProcessScan<'a> {
    pub proc_root: &'a [u8],
    pub page_size: u64,
    pub online_cores: u64,
    pub delta_global_ticks: u64,
    pub stat_buf: &'a mut Vec<u8>,
    pub path_buf: &'a mut Vec<u8>,
}

pub(super) fn read_stat(
    scan: &mut ProcessScan<'_>,
    name: &[u8],
    truncated: &mut bool,
) -> Option<ProcessProcStat> {
    scan.path_buf.clear();
    scan.path_buf.extend_from_slice(scan.proc_root);
    scan.path_buf.push(b'/');
    scan.path_buf.extend_from_slice(name);
    scan.path_buf.extend_from_slice(b"/stat\0");
    // SAFETY: path_buf is NUL-terminated and outlives the call.
    let fd = unsafe {
        libc::open(
            scan.path_buf.as_ptr() as *const libc::c_char,
            libc::O_RDONLY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        *truncated = true;
        return None;
    }
    let guard = FdGuard(fd);
    scan.stat_buf.clear();
    loop {
        let len = scan.stat_buf.len();
        let cap = scan.stat_buf.capacity();
        if len == cap {
            *truncated = true;
            return None;
        }
        // SAFETY: the read writes into spare capacity [len, cap); set_len runs
        // only after that many bytes were actually written.
        let n = unsafe {
            libc::read(
                guard.0,
                scan.stat_buf.as_mut_ptr().add(len) as *mut libc::c_void,
                cap - len,
            )
        };
        if n < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            *truncated = true;
            return None;
        }
        if n == 0 {
            break;
        }
        // SAFETY: n bytes were just written past len.
        unsafe { scan.stat_buf.set_len(len + n as usize) };
    }
    match parse_proc_stat(scan.stat_buf) {
        Some(stat) => Some(stat),
        None => {
            *truncated = true;
            None
        }
    }
}

pub(super) fn parse_dirent(
    buf: &[u8; DIRENT_BUF_LEN],
    pos: usize,
    nread: usize,
) -> AuraResult<(&[u8], usize)> {
    if pos + DIRENT_HEADER_LEN > nread {
        return Err(AuraError::Fatal("truncated dirent header".to_string()));
    }
    let reclen = u16::from_ne_bytes([buf[pos + 16], buf[pos + 17]]) as usize;
    if reclen < DIRENT_HEADER_LEN || pos + reclen > nread {
        return Err(AuraError::Fatal("corrupt dirent record length".to_string()));
    }
    let name_raw = &buf[pos + DIRENT_HEADER_LEN..pos + reclen];
    let len = name_raw
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| AuraError::Fatal("unterminated dirent name".to_string()))?;
    let d_type = buf[pos + 18];
    if d_type != libc::DT_DIR && d_type != libc::DT_UNKNOWN && d_type != libc::DT_LNK {
        return Ok((&[], reclen));
    }
    Ok((&name_raw[..len], reclen))
}

pub(super) fn parse_pid(name: &[u8]) -> Option<u64> {
    if name.is_empty() || name.len() > 10 {
        return None;
    }
    let mut value = 0u64;
    for &byte in name {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u64::from(byte - b'0');
    }
    if value == 0 || value > u64::from(MAX_PID) {
        return None;
    }
    Some(value)
}

pub(super) struct RawDir(libc::c_int);

impl RawDir {
    pub(super) fn open(proc_root: &[u8], path_buf: &mut Vec<u8>) -> AuraResult<Self> {
        path_buf.clear();
        path_buf.extend_from_slice(proc_root);
        path_buf.push(0);
        // SAFETY: path_buf is NUL-terminated and outlives the call.
        let fd = unsafe {
            libc::open(
                path_buf.as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY,
            )
        };
        if fd < 0 {
            return Err(AuraError::Fatal(format!(
                "open proc root: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self(fd))
    }
}

impl ProcessDirectory for RawDir {
    fn read(&mut self, buf: &mut [u8; DIRENT_BUF_LEN]) -> AuraResult<usize> {
        loop {
            // SAFETY: buf is writable for its full length; fd is an open directory.
            // Pinned libc exposes no getdents64 wrapper, so the raw syscall is used.
            let n = unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    self.0,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(AuraError::Fatal(format!("getdents64: {err}")));
            }
            return Ok(n as usize);
        }
    }
}

impl Drop for RawDir {
    fn drop(&mut self) {
        // SAFETY: fd is owned by self and closed exactly once here.
        let _ = unsafe { libc::close(self.0) };
    }
}

struct FdGuard(libc::c_int);

impl Drop for FdGuard {
    fn drop(&mut self) {
        // SAFETY: fd is owned by self and closed exactly once here.
        let _ = unsafe { libc::close(self.0) };
    }
}
