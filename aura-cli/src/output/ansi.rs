use crate::ColorMode;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";

pub const FG_RED: &str = "\x1b[31m";
pub const FG_GREEN: &str = "\x1b[32m";
pub const FG_YELLOW: &str = "\x1b[33m";
pub const FG_MAGENTA: &str = "\x1b[35m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Red,
    Yellow,
    Magenta,
    Green,
}

pub fn cpu_color(usage: f32) -> Tone {
    if usage > 80.0 {
        Tone::Red
    } else if usage > 70.0 {
        Tone::Yellow
    } else if usage > 60.0 {
        Tone::Magenta
    } else {
        Tone::Green
    }
}

pub fn memory_color(percent: f32) -> Tone {
    cpu_color(percent)
}

pub fn temperature_color(temp: i16) -> Tone {
    if temp >= 80 {
        Tone::Red
    } else if temp >= 70 {
        Tone::Yellow
    } else if temp >= 60 {
        Tone::Magenta
    } else {
        Tone::Green
    }
}

pub fn fmt_pct(mode: ColorMode, value: f32, tone: Tone) -> String {
    paint(mode, &format!("{:>6.1}%", value), tone)
}

pub fn paint(mode: ColorMode, text: &str, tone: Tone) -> String {
    match mode {
        ColorMode::None => text.to_string(),
        ColorMode::Ansi | ColorMode::Tmux | ColorMode::Zellij => {
            let fg = tone_code(tone);
            format!("{fg}{text}{RESET}")
        }
    }
}

pub fn style(mode: ColorMode, control: &str, text: &str) -> String {
    match mode {
        ColorMode::None => text.to_string(),
        ColorMode::Ansi | ColorMode::Tmux | ColorMode::Zellij => {
            format!("{control}{text}{RESET}")
        }
    }
}

fn tone_code(tone: Tone) -> &'static str {
    match tone {
        Tone::Red => FG_RED,
        Tone::Yellow => FG_YELLOW,
        Tone::Magenta => FG_MAGENTA,
        Tone::Green => FG_GREEN,
    }
}

pub fn fmt_bps(bytes_per_sec: f32) -> String {
    if bytes_per_sec >= 1_000_000_000.0 {
        format!("{:.1} GB/s", bytes_per_sec / 1_000_000_000.0)
    } else if bytes_per_sec >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 1_000.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1_000.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

pub fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use crate::ColorMode;

    use super::{cpu_color, fmt_pct, memory_color, temperature_color, Tone};

    #[test]
    fn cpu_thresholds_follow_expected_ranges() {
        assert_eq!(cpu_color(95.0), Tone::Red);
        assert_eq!(cpu_color(82.0), Tone::Red);
        assert_eq!(cpu_color(73.0), Tone::Yellow);
        assert_eq!(cpu_color(10.0), Tone::Green);
    }

    #[test]
    fn memory_thresholds_follow_expected_ranges() {
        assert_eq!(memory_color(90.0), Tone::Red);
        assert_eq!(memory_color(80.0), Tone::Yellow);
        assert_eq!(memory_color(69.0), Tone::Magenta);
        assert_eq!(memory_color(50.0), Tone::Green);
    }

    #[test]
    fn temperature_thresholds_follow_expected_ranges() {
        assert_eq!(temperature_color(85), Tone::Red);
        assert_eq!(temperature_color(75), Tone::Yellow);
        assert_eq!(temperature_color(65), Tone::Magenta);
        assert_eq!(temperature_color(50), Tone::Green);
    }

    #[test]
    fn fmt_pct_plain_when_color_disabled() {
        let out = fmt_pct(ColorMode::None, 12.3, Tone::Green);
        assert_eq!(out, "  12.3%");
    }
}
