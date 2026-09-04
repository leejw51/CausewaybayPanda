//! USDC amounts as integer micro-units (6 decimals), same as Causewaybay Wallet.

pub const MICRO: i64 = 1_000_000;

pub fn format_usdc(micro: i64) -> String {
    let sign = if micro < 0 { "-" } else { "" };
    let n = micro.abs();
    let whole = n / MICRO;
    let frac = n % MICRO;
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let frac = format!("{frac:06}");
        let frac = frac.trim_end_matches('0');
        format!("{sign}{whole}.{frac}")
    }
}

/// Parse "4.80" or "4" or "4.8" into micro-USDC. Rejects more than 6 decimals.
pub fn parse_usdc(text: &str) -> Option<i64> {
    let t = text.trim().trim_start_matches('$');
    let t = t
        .trim_end_matches("usdc")
        .trim()
        .trim_end_matches("USDC")
        .trim();
    if t.is_empty() {
        return None;
    }
    let neg = t.starts_with('-');
    let t = t.trim_start_matches('-').trim_start_matches('+');
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.is_empty() && !whole.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac.len() > 6 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let w: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut padded = frac.to_string();
    while padded.len() < 6 {
        padded.push('0');
    }
    let f: i64 = if padded.is_empty() {
        0
    } else {
        padded.parse().ok()?
    };
    let v = w.checked_mul(MICRO)?.checked_add(f)?;
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        assert_eq!(format_usdc(4_800_000), "4.8");
        assert_eq!(format_usdc(50_000_000), "50");
        assert_eq!(format_usdc(16_000), "0.016");
        assert_eq!(parse_usdc("4.80"), Some(4_800_000));
        assert_eq!(parse_usdc("4"), Some(4_000_000));
        assert_eq!(parse_usdc("3.20 USDC"), Some(3_200_000));
        assert_eq!(parse_usdc("0.5"), Some(500_000));
        assert_eq!(parse_usdc("not"), None);
        assert_eq!(parse_usdc("1.1234567"), None);
        assert_eq!(parse_usdc(""), None);
        assert_eq!(parse_usdc("-1.5"), Some(-1_500_000));
        assert_eq!(format_usdc(-2_000_000), "-2");
    }
}
