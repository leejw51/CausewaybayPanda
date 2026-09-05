//! Everything about how this particular cafe runs: which till, what the board
//! reads in, and who is listening to the chat.
//!
//! ```text
//! PANDA_MODE=simulation|live    # simulation is the default
//! PANDA_DENOM=HKD               # what prices read as
//! PANDA_DENOM_RATE=7.8          # units per USDC, if the built-in is stale
//! ```
//!
//! Simulation is the shop as a demonstration: guests are handed Causewaybay
//! Coin, there is a faucet, and nothing touches a chain. Live is the shop
//! taking real USDC on Cronos. A shop asked to run live without a treasury and
//! a token stays in simulation and says why, because a cafe that will not open
//! is worse than a cafe that cannot bank.

use causewaybay_panda_protocol::denom::{Denom, RATE_SCALE};
use causewaybay_panda_protocol::wire::{DenomView, Mode, Settlement as WireSettlement};
use causewaybay_panda_protocol::{COIN_NAME, FAUCET_CAP, FAUCET_GRANT};

use crate::settlement::Settle;

#[derive(Debug, Clone)]
pub struct Shop {
    pub mode: Mode,
    pub denom: Denom,
    pub settle: Settle,
    /// Why the shop is not live, when it was asked to be. Empty otherwise.
    pub mode_reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub mode: Option<String>,
    pub denom: Option<String>,
    pub rate: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
}

impl Config {
    pub fn from_env() -> Self {
        let get = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        Self {
            mode: get("PANDA_MODE"),
            denom: get("PANDA_DENOM"),
            rate: get("PANDA_DENOM_RATE"),
            symbol: get("PANDA_DENOM_SYMBOL"),
            decimals: get("PANDA_DENOM_DECIMALS").and_then(|s| s.parse().ok()),
        }
    }
}

impl Shop {
    pub fn from_env(settle: Settle) -> Self {
        Self::resolve(&Config::from_env(), settle)
    }

    pub fn resolve(cfg: &Config, settle: Settle) -> Self {
        let denom = resolve_denom(cfg);
        let wants_live = matches!(
            cfg.mode
                .as_deref()
                .map(str::trim)
                .map(str::to_lowercase)
                .as_deref(),
            Some("live") | Some("real") | Some("chain")
        );
        let (mode, mode_reason) = if !wants_live {
            (Mode::Simulation, String::new())
        } else if settle.onchain() {
            (Mode::Live, String::new())
        } else {
            (
                Mode::Simulation,
                format!("asked for live but {}", settle.reason()),
            )
        };
        Self {
            mode,
            denom,
            settle,
            mode_reason,
        }
    }

    /// A shop with nothing configured: test money, a faucet, no chain.
    pub fn simulation() -> Self {
        Self {
            mode: Mode::Simulation,
            denom: Denom::default(),
            settle: Settle::demo(),
            mode_reason: String::new(),
        }
    }

    pub fn is_simulation(&self) -> bool {
        self.mode.is_simulation()
    }

    /// Real USDC may only move when the shop is actually live.
    pub fn onchain(&self) -> bool {
        !self.is_simulation() && self.settle.onchain()
    }

    /// The faucet exists in simulation and nowhere else.
    pub fn can_faucet(&self, balance_micro: i64) -> bool {
        self.is_simulation() && balance_micro < FAUCET_CAP
    }

    /// What the board says, e.g. "HK$38.00".
    pub fn price(&self, micro: i64) -> String {
        self.denom.price(micro)
    }

    /// Why a wallet payment is not on offer.
    pub fn onchain_reason(&self) -> String {
        if self.is_simulation() {
            if self.mode_reason.is_empty() {
                format!("this shop is running in simulation on {COIN_NAME}")
            } else {
                self.mode_reason.clone()
            }
        } else {
            self.settle.reason().to_string()
        }
    }

    pub fn wire(&self) -> WireSettlement {
        let mut w = self.settle.wire();
        w.mode = self.mode;
        w.coin_name = COIN_NAME.into();
        w.onchain = self.onchain();
        if !w.onchain {
            w.usdc_address = String::new();
            w.reason = self.onchain_reason();
        }
        w.denom = DenomView {
            code: self.denom.code.clone(),
            symbol: self.denom.symbol.clone(),
            decimals: self.denom.decimals,
            rate: format_rate(self.denom.rate_milli),
        };
        w.faucet_display = self.denom.price(FAUCET_GRANT);
        w.faucet_cap_display = self.denom.price(FAUCET_CAP);
        w
    }
}

fn resolve_denom(cfg: &Config) -> Denom {
    let mut d = cfg
        .denom
        .as_deref()
        .and_then(Denom::preset)
        .unwrap_or_else(|| match cfg.denom.as_deref() {
            // A code we have never heard of still works, as long as its rate
            // is given; without one there is nothing to convert with.
            Some(code) if cfg.rate.is_some() => Denom::custom(code, code, RATE_SCALE, 2),
            _ => Denom::default(),
        });
    if let Some(rate) = cfg.rate.as_deref().and_then(parse_rate) {
        d.rate_milli = rate;
    }
    if let Some(sym) = cfg.symbol.clone() {
        d.symbol = sym;
    }
    if let Some(dp) = cfg.decimals {
        d.decimals = dp.min(6);
    }
    d
}

/// "7.8" → 7800. Up to three decimal places, which is as fine as a shop rate
/// ever needs to be.
fn parse_rate(text: &str) -> Option<i64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t, ""),
    };
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac.len() > 3 || (whole.is_empty() && frac.is_empty()) {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let frac: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<3}").parse().ok()?
    };
    let milli = whole.checked_mul(RATE_SCALE)?.checked_add(frac)?;
    (milli > 0).then_some(milli)
}

fn format_rate(milli: i64) -> String {
    let whole = milli / RATE_SCALE;
    let frac = (milli % RATE_SCALE).abs();
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{:03}", frac)
            .trim_end_matches('0')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use causewaybay_panda_protocol::money::MICRO;
    use causewaybay_panda_protocol::seed::TREASURY as SEED_TREASURY;

    const REAL: &str = "0x1111111111111111111111111111111111111111";

    fn live_settle() -> Settle {
        Settle::resolve(
            &crate::settlement::Config {
                chain_key: Some("cronos_mainnet".into()),
                treasury: Some(REAL.into()),
                ..Default::default()
            },
            SEED_TREASURY,
        )
    }

    fn cfg(mode: Option<&str>, denom: Option<&str>) -> Config {
        Config {
            mode: mode.map(Into::into),
            denom: denom.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn a_shop_opens_in_simulation_on_hong_kong_dollars() {
        let s = Shop::resolve(&Config::default(), Settle::demo());
        assert!(s.is_simulation());
        assert!(!s.onchain());
        assert_eq!(s.denom.code, "HKD");
        assert_eq!(s.price(MICRO), "HK$7.80");
        assert!(s.can_faucet(0));
    }

    #[test]
    fn live_needs_a_treasury_and_says_so_when_it_has_none() {
        let s = Shop::resolve(&cfg(Some("live"), None), Settle::demo());
        assert!(s.is_simulation(), "a shop without a till still opens");
        assert!(
            s.mode_reason.contains("asked for live"),
            "{}",
            s.mode_reason
        );
        assert!(s.onchain_reason().contains("asked for live"));
        // And the page is told the truth.
        let w = s.wire();
        assert!(!w.onchain);
        assert_eq!(w.usdc_address, "");
    }

    #[test]
    fn live_with_a_treasury_takes_real_usdc_and_shuts_the_faucet() {
        let s = Shop::resolve(&cfg(Some("live"), None), live_settle());
        assert!(!s.is_simulation());
        assert!(s.onchain());
        assert!(!s.can_faucet(0), "no free money in a real shop");
        let w = s.wire();
        assert_eq!(w.mode, Mode::Live);
        assert_eq!(w.chain_id, 25);
        assert!(!w.usdc_address.is_empty());
    }

    #[test]
    fn a_chain_configured_shop_left_in_simulation_still_moves_no_money() {
        // Everything is set up, but nobody asked for live.
        let s = Shop::resolve(&Config::default(), live_settle());
        assert!(s.is_simulation());
        assert!(!s.onchain(), "simulation must never move real USDC");
        assert!(s.can_faucet(0));
        assert_eq!(s.wire().usdc_address, "");
        assert!(s.onchain_reason().contains("simulation"));
    }

    #[test]
    fn the_faucet_stops_at_the_cap() {
        let s = Shop::simulation();
        assert!(s.can_faucet(FAUCET_CAP - 1));
        assert!(!s.can_faucet(FAUCET_CAP));
        assert!(!s.can_faucet(FAUCET_CAP + MICRO));
    }

    #[test]
    fn the_board_can_read_in_won_or_yen() {
        let krw = Shop::resolve(&cfg(None, Some("KRW")), Settle::demo());
        assert_eq!(krw.denom.code, "KRW");
        assert_eq!(krw.price(MICRO), "₩1,380");
        assert_eq!(krw.wire().denom.rate, "1380");

        let jpy = Shop::resolve(&cfg(None, Some("jpy")), Settle::demo());
        assert_eq!(jpy.price(MICRO), "¥157");
    }

    #[test]
    fn an_operator_can_correct_a_stale_rate() {
        let mut c = cfg(None, Some("KRW"));
        c.rate = Some("1500".into());
        let s = Shop::resolve(&c, Settle::demo());
        assert_eq!(s.price(MICRO), "₩1,500");
        assert_eq!(s.wire().denom.rate, "1500");
    }

    #[test]
    fn a_denomination_we_do_not_ship_works_when_its_rate_is_given() {
        let mut c = cfg(None, Some("PHP"));
        c.rate = Some("58".into());
        c.symbol = Some("₱".into());
        let s = Shop::resolve(&c, Settle::demo());
        assert_eq!(s.price(MICRO), "₱58.00");

        // Without a rate there is nothing to convert with, so the shop keeps
        // its default board rather than inventing one.
        let s = Shop::resolve(&cfg(None, Some("PHP")), Settle::demo());
        assert_eq!(s.denom.code, "HKD");
    }

    #[test]
    fn rates_are_read_and_written_without_a_float() {
        assert_eq!(parse_rate("7.8"), Some(7_800));
        assert_eq!(parse_rate("1380"), Some(1_380_000));
        assert_eq!(parse_rate("0.925"), Some(925));
        assert_eq!(parse_rate("7.8000"), None, "more than milli is a typo");
        assert_eq!(parse_rate("0"), None, "a rate of nothing is not a rate");
        assert_eq!(parse_rate("-3"), None);
        assert_eq!(parse_rate("abc"), None);

        assert_eq!(format_rate(7_800), "7.8");
        assert_eq!(format_rate(1_380_000), "1380");
        assert_eq!(format_rate(925), "0.925");
    }
}
