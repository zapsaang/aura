#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FixedString16 {
    pub bytes: [u8; 16],
}

impl FixedString16 {
    pub const fn new() -> Self {
        Self { bytes: [0u8; 16] }
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        let mut s = Self::new();
        let end = find_utf8_truncation_point(b, 16);
        let mut i = 0;
        while i < end {
            s.bytes[i] = b[i];
            i += 1;
        }
        s
    }

    pub fn as_str(&self) -> &str {
        let len = self.bytes.iter().position(|&b| b == 0).unwrap_or(16);
        let slice = &self.bytes[..len];
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(e) => std::str::from_utf8(&slice[..e.valid_up_to()]).unwrap_or(""),
        }
    }
}

/// Convert a byte slice to a `String`, stopping at the first NUL byte.
/// Uses lossy UTF-8 conversion for any invalid sequences.
pub fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn find_utf8_truncation_point(b: &[u8], max_len: usize) -> usize {
    let len = b.len().min(max_len);
    if len == 0 {
        return 0;
    }

    let mut i = 0;
    while i < len {
        let byte = b[i];

        if byte & 0x80 == 0 {
            i += 1;
            continue;
        }

        let cont_needed = if (byte & 0xE0) == 0xC0 {
            1
        } else if (byte & 0xF0) == 0xE0 {
            2
        } else if (byte & 0xF8) == 0xF0 {
            3
        } else {
            i += 1;
            continue;
        };

        if i + cont_needed < len {
            i += cont_needed + 1;
        } else {
            return i;
        }
    }

    len
}

impl Default for FixedString16 {
    fn default() -> Self {
        Self::new()
    }
}
