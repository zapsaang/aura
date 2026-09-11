/// Decimal SI formatting: base 1000, suffix `B|KB|MB|GB|TB`, always one decimal.
pub fn si(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1_000_000_000_000.0 {
        format!("{:.1}TB", value / 1_000_000_000_000.0)
    } else if abs >= 1_000_000_000.0 {
        format!("{:.1}GB", value / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{:.1}MB", value / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}KB", value / 1_000.0)
    } else {
        format!("{value:.1}B")
    }
}

#[cfg(test)]
mod tests {
    use super::si;

    #[test]
    fn si_uses_decimal_base_with_one_decimal() {
        assert_eq!(si(0.0), "0.0B");
        assert_eq!(si(999.0), "999.0B");
        assert_eq!(si(1_000.0), "1.0KB");
        assert_eq!(si(1_500.0), "1.5KB");
        assert_eq!(si(2_000_000.0), "2.0MB");
        assert_eq!(si(16_000_000_000.0), "16.0GB");
        assert_eq!(si(3_000_000_000_000.0), "3.0TB");
    }
}
