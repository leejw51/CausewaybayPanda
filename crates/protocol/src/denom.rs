//! What a price *reads* as. Every amount in this system is micro-USDC — that
//! is what the cart holds, what the books record, and what moves on chain.
//! A denomination is presentation only: the Causeway Bay menu says HK$38, the
//! same coffee in Seoul says ₩5,300, and the transfer is the same 4.87 USDC.
//!
//! ```text
//! PANDA_DENOM=HKD               # HKD KRW JPY CNY TWD SGD EUR GBP USD USDC
//! PANDA_DENOM_RATE=7.8          # units per USDC, if the built-in is stale
//! PANDA_DENOM_SYMBOL=HK$
//! PANDA_DENOM_DECIMALS=2
//! ```
//!
//! The built-in rates are starting points, not a price feed. HKD is pegged to
//! the US dollar in a 7.75–7.85 band so it barely moves; every other rate here
//! drifts and a real shop should set `PANDA_DENOM_RATE` from its own source.

use crate::money::MICRO;

/// Rates are held to three decimal places as integers, so no float ever
/// touches a price.
pub const RATE_SCALE: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denom {
    pub code: String,
    pub symbol: String,
    /// Units of this denomination per 1 USDC, times `RATE_SCALE`.
    pub rate_milli: i64,
    /// Digits after the point. Won and yen have none.
    pub decimals: u8,
}

/// A built-in starting point. `rate_milli` is units per USDC × 1000.
struct Preset {
    code: &'static str,
    symbol: &'static str,
    rate_milli: i64,
    decimals: u8,
}

const PRESETS: &[Preset] = &[
    // Pegged to the dollar at 7.75–7.85, so this one really is stable.
    Preset {
        code: "HKD",
        symbol: "HK$",
        rate_milli: 7_800,
        decimals: 2,
    },
    Preset {
        code: "USD",
        symbol: "$",
        rate_milli: 1_000,
        decimals: 2,
    },
    // The raw settlement unit, for anyone who would rather see it.
    Preset {
        code: "USDC",
        symbol: "",
        rate_milli: 1_000,
        decimals: 2,
    },
    Preset {
        code: "KRW",
        symbol: "₩",
        rate_milli: 1_380_000,
        decimals: 0,
    },
    Preset {
        code: "JPY",
        symbol: "¥",
        rate_milli: 157_000,
        decimals: 0,
    },
    Preset {
        code: "CNY",
        symbol: "CN¥",
        rate_milli: 7_100,
        decimals: 2,
    },
    Preset {
        code: "TWD",
        symbol: "NT$",
        rate_milli: 32_000,
        decimals: 2,
    },
    Preset {
        code: "SGD",
        symbol: "S$",
        rate_milli: 1_350,
        decimals: 2,
    },
    Preset {
        code: "EUR",
        symbol: "€",
        rate_milli: 920,
        decimals: 2,
    },
    Preset {
        code: "GBP",
        symbol: "£",
        rate_milli: 790,
        decimals: 2,
    },
];

impl Default for Denom {
    fn default() -> Self {
        Self::preset("HKD").expect("HKD is a preset")
    }
}

impl Denom {
    pub fn preset(code: &str) -> Option<Self> {
        let want = code.trim().to_uppercase();
        PRESETS.iter().find(|p| p.code == want).map(|p| Self {
            code: p.code.to_string(),
            symbol: p.symbol.to_string(),
            rate_milli: p.rate_milli,
            decimals: p.decimals,
        })
    }

    pub fn codes() -> Vec<&'static str> {
        PRESETS.iter().map(|p| p.code).collect()
    }

    /// A denomination the presets have never heard of, named by its operator.
    pub fn custom(code: &str, symbol: &str, rate_milli: i64, decimals: u8) -> Self {
        Self {
            code: code.trim().to_uppercase(),
            symbol: symbol.to_string(),
            rate_milli: rate_milli.max(1),
            decimals: decimals.min(6),
        }
    }

    fn pow10(&self) -> i128 {
        10i128.pow(self.decimals as u32)
    }

    /// micro-USDC → this denomination's smallest unit (cents, or whole won).
    pub fn minor_from_micro(&self, micro: i64) -> i128 {
        let n = micro as i128 * self.rate_milli as i128 * self.pow10();
        let d = MICRO as i128 * RATE_SCALE as i128;
        round_div(n, d)
    }

    /// This denomination's smallest unit → micro-USDC. The inverse of
    /// `minor_from_micro`, to the nearest micro.
    pub fn micro_from_minor(&self, minor: i128) -> i64 {
        let n = minor * MICRO as i128 * RATE_SCALE as i128;
        let d = self.rate_milli as i128 * self.pow10();
        round_div(n, d).clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// "38.00" — the number alone, for an input box.
    pub fn amount(&self, micro: i64) -> String {
        let minor = self.minor_from_micro(micro);
        let neg = minor < 0;
        let minor = minor.unsigned_abs();
        let p = self.pow10() as u128;
        let whole = minor / p;
        let sign = if neg { "-" } else { "" };
        if self.decimals == 0 {
            return format!("{sign}{}", group(whole));
        }
        let frac = minor % p;
        format!(
            "{sign}{}.{:0width$}",
            group(whole),
            frac,
            width = self.decimals as usize
        )
    }

    /// "HK$38.00" — what a guest reads on the board.
    pub fn price(&self, micro: i64) -> String {
        let amount = self.amount(micro);
        if self.symbol.is_empty() {
            format!("{amount} {}", self.code)
        } else {
            format!("{}{amount}", self.symbol)
        }
    }

    /// Read what someone typed into a price box: "38", "HK$38.00", "38.5".
    pub fn parse(&self, text: &str) -> Option<i64> {
        let mut t = text.trim().to_string();
        for strip in [self.symbol.as_str(), self.code.as_str(), "$", ","] {
            if !strip.is_empty() {
                t = t.replace(strip, "");
            }
        }
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        // Exactly one sign, so "--3" is a typo rather than a negative three.
        let (neg, t) = match t.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        let t = t.trim();
        let (whole, frac) = match t.split_once('.') {
            Some((w, f)) => (w, f),
            None => (t, ""),
        };
        if whole.is_empty() && frac.is_empty() {
            return None;
        }
        if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        // More precision than the denomination has is a typo, not a price.
        if frac.len() > self.decimals as usize {
            return None;
        }
        let whole: i128 = if whole.is_empty() {
            0
        } else {
            whole.parse().ok()?
        };
        let mut minor = whole.checked_mul(self.pow10())?;
        if !frac.is_empty() {
            let pad = format!("{frac:0<width$}", width = self.decimals as usize);
            minor = minor.checked_add(pad.parse::<i128>().ok()?)?;
        }
        let micro = self.micro_from_minor(if neg { -minor } else { minor });
        Some(micro)
    }
}

/// Divide and round half away from zero, so a half-cent lands on the shop's
/// side rather than vanishing. Every divisor here is a positive scale factor.
fn round_div(n: i128, d: i128) -> i128 {
    if d <= 0 {
        return 0;
    }
    if n >= 0 {
        (n + d / 2) / d
    } else {
        (n - d / 2) / d
    }
}

/// Thousands separators, because ₩5,300 is easier to read than ₩5300.
fn group(n: u128) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hong_kong_dollars_are_the_default_board() {
        let d = Denom::default();
        assert_eq!(d.code, "HKD");
        assert_eq!(d.symbol, "HK$");
        // 1 USDC is HK$7.80.
        assert_eq!(d.price(MICRO), "HK$7.80");
    }

    #[test]
    fn a_price_survives_the_round_trip_in_every_preset() {
        for code in Denom::codes() {
            let d = Denom::preset(code).unwrap();
            for text in ["38", "5", "1"] {
                let micro = d.parse(text).unwrap();
                let back = d.amount(micro);
                let again = d.parse(&back).unwrap();
                assert_eq!(
                    d.amount(again),
                    back,
                    "{code}: {text} -> {micro} -> {back} drifted"
                );
            }
        }
    }

    #[test]
    fn the_same_coffee_reads_differently_and_settles_the_same() {
        // One latte, priced by a Hong Kong owner at HK$38.
        let hkd = Denom::default();
        let micro = hkd.parse("38").unwrap();
        assert_eq!(hkd.price(micro), "HK$38.00");

        // The guest's phone is set to won; the transfer is unchanged.
        let krw = Denom::preset("KRW").unwrap();
        assert_eq!(krw.price(micro), "₩6,723");

        // And in the settlement unit itself.
        let usdc = Denom::preset("USDC").unwrap();
        assert_eq!(usdc.price(micro), "4.87 USDC");
        assert_eq!(micro, 4_871_795);
    }

    #[test]
    fn currencies_without_a_minor_unit_show_no_point() {
        let krw = Denom::preset("KRW").unwrap();
        assert_eq!(krw.decimals, 0);
        assert_eq!(krw.price(MICRO), "₩1,380");
        // A fractional won is not a price.
        assert!(krw.parse("5300.5").is_none());
        assert_eq!(krw.parse("5,300"), krw.parse("5300"));

        let jpy = Denom::preset("JPY").unwrap();
        assert_eq!(jpy.price(MICRO), "¥157");
    }

    #[test]
    fn large_amounts_are_grouped() {
        let krw = Denom::preset("KRW").unwrap();
        assert_eq!(krw.amount(1_000 * MICRO), "1,380,000");
        let hkd = Denom::default();
        assert_eq!(hkd.amount(1_000 * MICRO), "7,800.00");
    }

    #[test]
    fn a_symbol_or_a_code_in_the_box_is_forgiven() {
        let d = Denom::default();
        let want = d.parse("38.00").unwrap();
        for text in ["HK$38", "HK$38.00", " 38.00 ", "38.0", "HKD 38"] {
            assert_eq!(d.parse(text), Some(want), "{text}");
        }
    }

    #[test]
    fn nonsense_in_a_price_box_is_refused() {
        let d = Denom::default();
        for text in ["", "   ", "free", "3.8.1", "12a", "--3", "38.999"] {
            assert!(d.parse(text).is_none(), "{text} should not parse");
        }
    }

    #[test]
    fn zero_and_negative_amounts_are_written_plainly() {
        let d = Denom::default();
        assert_eq!(d.amount(0), "0.00");
        assert_eq!(d.price(0), "HK$0.00");
        assert_eq!(d.amount(-MICRO), "-7.80");
    }

    #[test]
    fn an_operator_can_name_a_denomination_we_have_never_heard_of() {
        let d = Denom::custom("php", "₱", 58_000, 2);
        assert_eq!(d.code, "PHP");
        assert_eq!(d.price(MICRO), "₱58.00");
        assert_eq!(d.parse("58").unwrap(), MICRO);
        assert!(Denom::preset("PHP").is_none());
    }

    #[test]
    fn rounding_is_half_away_from_zero_so_the_shop_is_never_short() {
        let d = Denom::default();
        assert_eq!(d.minor_from_micro(0), 0);
        assert_eq!(d.minor_from_micro(MICRO), 780);

        // In the settlement unit a cent is exactly 10_000 micro, so half of
        // one is exact and shows which way the rounding goes.
        let usdc = Denom::preset("USDC").unwrap();
        assert_eq!(usdc.minor_from_micro(10_000), 1);
        assert_eq!(usdc.minor_from_micro(5_000), 1, "half a cent rounds up");
        assert_eq!(usdc.minor_from_micro(4_999), 0);
        assert_eq!(usdc.minor_from_micro(-5_000), -1, "and away from zero");
    }
}
