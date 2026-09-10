use aura_common::{AuraError, AuraResult, FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};

use crate::collectors::state::NetIfKey;
use crate::collectors::NetworkAvailability;

/// Darwin `RTM_VERSION` required on every routing message.
pub const RTM_VERSION: u8 = 5;
/// Darwin `RTM_IFINFO2`: the only message type carrying interface counters.
pub const RTM_IFINFO2: u8 = 18;
/// Darwin AF_LINK address family selector for `sockaddr_dl`.
pub const AF_LINK: u8 = 18;
/// `RTA_IFP` bitmask inside `ifm_addrs` (RTAX_IFP is slot 4).
pub const RTA_IFP: i32 = 0x10;
/// Size of the fixed `if_msghdr2` header preceding the sockaddr chain.
pub const IF_MSGHDR2_LEN: usize = 224;
/// Offset of `ifm_data.ifi_ibytes` inside `if_msghdr2`.
pub const IFM_IBYTES_OFFSET: usize = 152;
/// Offset of `ifm_data.ifi_obytes` inside `if_msghdr2`.
pub const IFM_OBYTES_OFFSET: usize = 160;
/// Hard cap for the one init-time NET_RT_IFLIST2 buffer: 1 MiB.
pub const IFLIST2_CAPACITY_MAX: usize = 1024 * 1024;
/// Darwin ENOMEM: cycle-local network capability loss without truncation.
pub const MACOS_ENOMEM: i32 = 12;

const RT_MSG_PREFIX_LEN: usize = 4;
const RTAX_MAX: u32 = 8;
const RTAX_IFP: u32 = 4;
const IFM_ADDRS_OFFSET: usize = 16;
const SOCKADDR_ALIGN: usize = 8;
const SDL_HEADER_LEN: usize = 8;
const SDL_MAX_NAME: usize = 16;

/// Raw NET_RT_IFLIST2 routing dump query. The trait exposes only the stable
/// bytes written into the fixed init-time buffer; this module owns parsing,
/// ordering, truncation, and capability choices.
pub trait MacosNetworkProbe {
    /// Reuses the one fixed init-time routing buffer (never grown) and
    /// returns the valid dump bytes, or the raw errno (ENOMEM when the
    /// kernel needs more than the fixed capacity).
    fn iflist2_dump(&mut self) -> Result<&[u8], i32>;
}

fn fatal(message: String) -> AuraError {
    AuraError::Fatal(message)
}

/// Fixed buffer capacity from the init-time count query: one page of
/// headroom, rounded up to a page, clamped to [page, 1 MiB].
pub fn iflist2_capacity(needed: usize, page: usize) -> usize {
    let page = page.max(1);
    let want = needed.saturating_add(page);
    let rounded = want
        .checked_add(page - 1)
        .map_or(IFLIST2_CAPACITY_MAX, |value| value / page * page);
    rounded.clamp(page, IFLIST2_CAPACITY_MAX)
}

/// Init-time sizing and allocation of the single routing buffer. A failed
/// count query is Fatal; the platform layer owns the returned allocation
/// for the process lifetime.
pub fn init_iflist2_buffer(query: Result<usize, i32>, page: usize) -> AuraResult<Vec<u8>> {
    let needed =
        query.map_err(|code| fatal(format!("NET_RT_IFLIST2 count query failed: errno {code}")))?;
    Ok(vec![0u8; iflist2_capacity(needed, page)])
}

/// Darwin routing-message sockaddr stride: max(sa_len, sizeof(long))
/// rounded up to sizeof(long); sizeof(long) is 8 on every supported macOS.
const fn sockaddr_stride(sa_len: usize) -> usize {
    let floor = if sa_len < SOCKADDR_ALIGN {
        SOCKADDR_ALIGN
    } else {
        sa_len
    };
    (floor + SOCKADDR_ALIGN - 1) / SOCKADDR_ALIGN * SOCKADDR_ALIGN
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

/// Validates one RTA_IFP sockaddr_dl: AF_LINK family, nonzero sdl_index,
/// and a 1..=16 byte clean UTF-8 name inside the declared record.
fn parse_sdl(sdl: &[u8]) -> Option<(u32, &[u8])> {
    if sdl.len() < SDL_HEADER_LEN || sdl[1] != AF_LINK {
        return None;
    }
    let index = u32::from(read_u16(sdl, 2));
    if index == 0 {
        return None;
    }
    let name_len = sdl[5] as usize;
    if name_len == 0 || name_len > SDL_MAX_NAME || SDL_HEADER_LEN + name_len > sdl.len() {
        return None;
    }
    let name = &sdl[SDL_HEADER_LEN..SDL_HEADER_LEN + name_len];
    let text = std::str::from_utf8(name).ok()?;
    if text.bytes().any(|byte| byte == 0 || byte < 0x20) {
        return None;
    }
    Some((index, name))
}

/// Parses one RTM_IFINFO2 record: inline if_data64 counters plus the
/// sockaddr chain bounds-walked in RTAX order. The validated RTA_IFP
/// sockaddr_dl yields the (sdl_index, name) identity. Returns None when
/// the record must be skipped with truncation (disappearance, denial, or
/// malformed record fields).
fn parse_ifinfo2(record: &[u8]) -> Option<(u32, &[u8], u64, u64)> {
    if record.len() < IF_MSGHDR2_LEN {
        return None;
    }
    let addrs = read_u32(record, IFM_ADDRS_OFFSET) as i32;
    let rx = read_u64(record, IFM_IBYTES_OFFSET);
    let tx = read_u64(record, IFM_OBYTES_OFFSET);
    let mut offset = IF_MSGHDR2_LEN;
    for bit in 0..RTAX_MAX {
        if addrs & (1i32 << bit) == 0 {
            continue;
        }
        if offset + 2 > record.len() {
            return None;
        }
        let sa_len = record[offset] as usize;
        if sa_len < 2 {
            return None;
        }
        let stride = sockaddr_stride(sa_len);
        if offset + stride > record.len() {
            return None;
        }
        if bit == RTAX_IFP {
            let (index, name) = parse_sdl(&record[offset..offset + sa_len])?;
            return Some((index, name, rx, tx));
        }
        offset += stride;
    }
    None
}

/// Walks one NET_RT_IFLIST2 dump in routing-message order into the fixed
/// 16-slot table keyed by (sdl_index, name). Global structural violations
/// (trailing partial header, zero or oversized msglen, wrong RTM_VERSION)
/// are Fatal; per-record problems skip the record and set truncation.
pub fn parse_iflist2(
    dump: &[u8],
    out: &mut NetworkStats,
    keys: &mut [NetIfKey; MAX_NETIFS],
) -> AuraResult<()> {
    let mut count = 0usize;
    let mut truncated = false;
    let mut pos = 0usize;
    while pos < dump.len() {
        let remaining = dump.len() - pos;
        if remaining < RT_MSG_PREFIX_LEN {
            return Err(fatal(
                "NET_RT_IFLIST2 dump ends inside a message header".to_string(),
            ));
        }
        let message = &dump[pos..];
        let msglen = usize::from(read_u16(message, 0));
        if msglen == 0 || msglen > remaining {
            return Err(fatal(format!(
                "NET_RT_IFLIST2 message length {msglen} exceeds remaining {remaining}"
            )));
        }
        if message[2] != RTM_VERSION {
            return Err(fatal(format!(
                "NET_RT_IFLIST2 message version {} is not RTM_VERSION",
                message[2]
            )));
        }
        if message[3] != RTM_IFINFO2 {
            pos += msglen;
            continue;
        }
        match parse_ifinfo2(&message[..msglen]) {
            Some((index, name, rx, tx)) => {
                let key = NetIfKey::from_macos(index, name);
                if keys[..count].iter().any(|seen| seen.matches(&key)) {
                    // Duplicate (index, name): the first record wins.
                } else if count >= MAX_NETIFS {
                    truncated = true;
                } else {
                    out.interfaces[count] = NetIfStat {
                        name: FixedString16::from_bytes(name),
                        rx_bytes: rx,
                        tx_bytes: tx,
                        rx_bytes_per_sec: 0.0,
                        tx_bytes_per_sec: 0.0,
                    };
                    keys[count] = key;
                    count += 1;
                }
            }
            None => truncated = true,
        }
        pos += msglen;
    }
    out.if_count = count as u8;
    out.truncated = u8::from(truncated);
    Ok(())
}

/// Collects interface byte counters from one NET_RT_IFLIST2 dump. ENOMEM
/// (the kernel needs more than the fixed init-time capacity) clears the
/// network local capability with a zeroed table and no truncation or
/// partial publication; other dump failures are Fatal.
pub fn collect_network_from_probe<P: MacosNetworkProbe + ?Sized>(
    probe: &mut P,
    out: &mut NetworkStats,
    keys: &mut [NetIfKey; MAX_NETIFS],
) -> AuraResult<NetworkAvailability> {
    let dump = match probe.iflist2_dump() {
        Ok(dump) => dump,
        Err(MACOS_ENOMEM) => {
            *out = zero_network();
            for key in keys.iter_mut() {
                *key = NetIfKey::empty();
            }
            return Ok(NetworkAvailability {
                bytes: false,
                rates: false,
            });
        }
        Err(code) => {
            return Err(fatal(format!("NET_RT_IFLIST2 dump failed: errno {code}")));
        }
    };

    *out = zero_network();
    for key in keys.iter_mut() {
        *key = NetIfKey::empty();
    }
    parse_iflist2(dump, out, keys)?;

    Ok(NetworkAvailability {
        bytes: true,
        rates: true,
    })
}

const fn zero_network() -> NetworkStats {
    NetworkStats {
        interfaces: [NetIfStat::new(); MAX_NETIFS],
        if_count: 0,
        truncated: 0,
        _pad0: [0; 6],
    }
}

#[cfg(target_os = "macos")]
pub fn collect_with_keys(
    out: &mut NetworkStats,
    keys: &mut [NetIfKey; MAX_NETIFS],
) -> AuraResult<NetworkAvailability> {
    let mut host = crate::platform::macos::host()?;
    collect_network_from_probe(&mut host, out, keys)
}

#[cfg(target_os = "macos")]
pub fn collect(_buf: &mut Vec<u8>, out: &mut NetworkStats) -> AuraResult<()> {
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_with_keys(out, &mut keys).map(|_| ())
}
