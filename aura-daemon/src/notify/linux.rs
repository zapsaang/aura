use std::ffi::OsStr;

use aura_common::AuraResult;

use super::fatal;

pub(super) fn parse_decimal(name: &str, value: &OsStr) -> AuraResult<u64> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return Err(fatal(&format!(
            "{name} must be an unsigned decimal integer"
        )));
    }
    bytes.iter().try_fold(0_u64, |number, byte| {
        let digit = byte
            .checked_sub(b'0')
            .ok_or_else(|| fatal(&format!("{name} must be an unsigned decimal integer")))?;
        if digit > 9 {
            return Err(fatal(&format!(
                "{name} must be an unsigned decimal integer"
            )));
        }
        number
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(u64::from(digit)))
            .ok_or_else(|| fatal(&format!("{name} is outside the supported integer range")))
    })
}

struct NotifyAddress {
    raw: libc::sockaddr_un,
    length: libc::socklen_t,
}

impl NotifyAddress {
    fn parse(value: &OsStr) -> AuraResult<Self> {
        use std::os::unix::ffi::OsStrExt;

        let bytes = value.as_bytes();
        let path_offset = sun_path_offset();
        let capacity = std::mem::size_of::<libc::sockaddr_un>()
            .checked_sub(path_offset)
            .ok_or_else(|| fatal("sockaddr_un layout has no sun_path capacity"))?;
        let (source, offset, payload_length) = match bytes.first() {
            Some(b'/') if bytes.len() < capacity => (bytes, 0, bytes.len() + 1),
            Some(b'@') if bytes.len() > 1 && bytes.len() <= capacity => {
                (&bytes[1..], 1, bytes.len())
            }
            Some(b'/') | Some(b'@') => {
                return Err(fatal("NOTIFY_SOCKET does not fit sockaddr_un.sun_path"));
            }
            Some(_) | None => {
                return Err(fatal("NOTIFY_SOCKET must start with '/' or '@'"));
            }
        };
        if bytes.contains(&0) {
            return Err(fatal("NOTIFY_SOCKET must not contain an embedded NUL"));
        }
        // SAFETY: [Categories 4 and 5 — initialization and valid values]
        // Linux sockaddr_un permits an all-zero representation before its family and path are set.
        let mut raw = unsafe { std::mem::zeroed::<libc::sockaddr_un>() };
        raw.sun_family = libc::sa_family_t::try_from(libc::AF_UNIX)
            .map_err(|_| fatal("AF_UNIX does not fit sa_family_t"))?;
        let destination = raw.sun_path.as_mut_ptr().cast::<u8>().wrapping_add(offset);
        // SAFETY: [Categories 8 and 10 — FFI boundary and bounds]
        // `source.len()` was bounded by sun_path capacity above, and `offset` reserves only
        // the abstract namespace's required leading NUL inside the same array.
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr(), destination, source.len()) };
        let length = path_offset
            .checked_add(payload_length)
            .and_then(|value| libc::socklen_t::try_from(value).ok())
            .ok_or_else(|| fatal("NOTIFY_SOCKET address length overflow"))?;
        Ok(Self { raw, length })
    }
}

fn sun_path_offset() -> usize {
    let uninitialized = std::mem::MaybeUninit::<libc::sockaddr_un>::uninit();
    let base = uninitialized.as_ptr();
    // SAFETY: [Categories 4 and 11 — initialization and provenance]
    // addr_of forms a raw field pointer without reading the uninitialized value;
    // both pointers retain provenance from the same sockaddr_un storage.
    let path = unsafe { std::ptr::addr_of!((*base).sun_path) };
    path as usize - base as usize
}

pub(super) struct LinuxTransport {
    socket: std::os::fd::OwnedFd,
    address: NotifyAddress,
}

impl LinuxTransport {
    pub(super) fn new(notify_socket: &OsStr) -> AuraResult<Self> {
        use std::os::fd::FromRawFd;

        let address = NotifyAddress::parse(notify_socket)?;
        // SAFETY: [Categories 8 and 13 — FFI and libc contract]
        // The constants form a valid AF_UNIX datagram socket request with close-on-exec ownership.
        let descriptor =
            unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
        if descriptor < 0 {
            return Err(fatal(&format!(
                "systemd notification socket creation failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        // SAFETY: [Categories 8 and 12 — FFI ownership and invalid free]
        // `socket` returned a fresh nonnegative owned descriptor, transferred exactly once here.
        let socket = unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptor) };
        Ok(Self { socket, address })
    }

    pub(super) fn send(&self, message: &[u8], label: &str) -> AuraResult<()> {
        use std::os::fd::AsRawFd;

        // SAFETY: [Categories 8, 10, and 13 — FFI, bounds, and libc contract]
        // The owned descriptor is live; message covers `len`; address is initialized and its
        // exact checked length covers only initialized family/path bytes required by AF_UNIX.
        let sent = unsafe {
            libc::sendto(
                self.socket.as_raw_fd(),
                message.as_ptr().cast::<libc::c_void>(),
                message.len(),
                0,
                std::ptr::addr_of!(self.address.raw).cast::<libc::sockaddr>(),
                self.address.length,
            )
        };
        if sent < 0 {
            return Err(fatal(&format!(
                "systemd notification {label} send failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        if usize::try_from(sent).ok() != Some(message.len()) {
            return Err(fatal(&format!(
                "systemd notification {label} send was truncated"
            )));
        }
        Ok(())
    }
}
