use aura_common::{TONE_MAGENTA, TONE_RED, TONE_YELLOW};

use crate::args::ColorMode;

pub const NA: &str = "N/A";
pub const ARRAY_NA: &str = "    N/A";
pub const ARRAY_NONE: &str = "    (none)";

fn tone_name(tone: u8) -> &'static str {
    match tone {
        TONE_MAGENTA => "magenta",
        TONE_YELLOW => "yellow",
        TONE_RED => "red",
        _ => "green",
    }
}

/// Wrap `text` (an already-substituted metric value plus its suffix) in the
/// terminal color for `tone`. `none` emits no escape; ansi/zellij emit SGR;
/// tmux emits format tokens. The reset always follows the suffix.
pub fn paint(mode: ColorMode, tone: u8, text: &str) -> String {
    match mode {
        ColorMode::None => text.to_string(),
        ColorMode::Ansi | ColorMode::Zellij => {
            let code = match tone {
                TONE_MAGENTA => 35,
                TONE_YELLOW => 33,
                TONE_RED => 31,
                _ => 32,
            };
            format!("\x1b[{code}m{text}\x1b[0m")
        }
        ColorMode::Tmux => format!("#[fg={}]{text}#[default]", tone_name(tone)),
    }
}

pub fn tone_str(tone: u8) -> &'static str {
    tone_name(tone)
}

#[cfg(test)]
mod tests {
    use aura_common::{TONE_GREEN, TONE_MAGENTA, TONE_RED, TONE_YELLOW};

    use super::{paint, tone_str};
    use crate::args::ColorMode;

    #[test]
    fn ansi_and_zellij_wrap_with_sgr() {
        assert_eq!(
            paint(ColorMode::Ansi, TONE_GREEN, "1.0%"),
            "\x1b[32m1.0%\x1b[0m"
        );
        assert_eq!(
            paint(ColorMode::Zellij, TONE_MAGENTA, "2.0%"),
            "\x1b[35m2.0%\x1b[0m"
        );
        assert_eq!(
            paint(ColorMode::Ansi, TONE_YELLOW, "3.0%"),
            "\x1b[33m3.0%\x1b[0m"
        );
        assert_eq!(
            paint(ColorMode::Ansi, TONE_RED, "4.0%"),
            "\x1b[31m4.0%\x1b[0m"
        );
    }

    #[test]
    fn tmux_wraps_with_format_tokens() {
        assert_eq!(
            paint(ColorMode::Tmux, TONE_GREEN, "1.0%"),
            "#[fg=green]1.0%#[default]"
        );
        assert_eq!(
            paint(ColorMode::Tmux, TONE_RED, "85C"),
            "#[fg=red]85C#[default]"
        );
    }

    #[test]
    fn none_mode_emits_no_escape() {
        assert_eq!(paint(ColorMode::None, TONE_RED, "9.0%"), "9.0%");
    }

    #[test]
    fn tone_names_cover_all_four_tones() {
        assert_eq!(tone_str(TONE_GREEN), "green");
        assert_eq!(tone_str(TONE_MAGENTA), "magenta");
        assert_eq!(tone_str(TONE_YELLOW), "yellow");
        assert_eq!(tone_str(TONE_RED), "red");
    }
}
