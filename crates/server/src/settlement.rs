//! What the till settles in. Play money by default; real USDC on Cronos once
//! a token and a treasury are both named.
//!
//! ```text
//! PANDA_CHAIN=cronos_mainnet        # or cronos_testnet (default), or 25 / 338
//! PANDA_TREASURY=0x…                # the shop's wallet; required for on-chain
//! PANDA_USDC_ADDRESS=0x…            # required on testnet, optional on mainnet
//! PANDA_USDC_DECIMALS=6             # only if the token is not 6-decimal
//! ```

use causewaybay_panda_protocol::chain::{by_key, chain_id_hex, Chain, CRONOS_TESTNET};
use causewaybay_panda_protocol::denom::Denom;
use causewaybay_panda_protocol::erc20::{encode_transfer_hex, parse_address};
use causewaybay_panda_protocol::seed::TREASURY as SEED_TREASURY;
use causewaybay_panda_protocol::wire::{DenomView, Mode, Settlement as WireSettlement};
use causewaybay_panda_protocol::COIN_NAME;
use causewaybay_panda_protocol::USDC_DECIMALS;

#[derive(Debug, Clone)]
pub struct Settle {
    pub chain: Chain,
    rpc_url: String,
    token: Option<[u8; 20]>,
    token_display: String,
    decimals: u8,
    treasury: Option<[u8; 20]>,
    treasury_display: String,
    reason: String,
}

/// The four inputs that decide how the till settles, read from the process
/// environment or handed in directly by a test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub chain_key: Option<String>,
    pub treasury: Option<String>,
    pub token: Option<String>,
    pub decimals: Option<u8>,
    pub rpc_url: Option<String>,
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
            chain_key: get("PANDA_CHAIN"),
            treasury: get("PANDA_TREASURY"),
            token: get("PANDA_USDC_ADDRESS"),
            decimals: get("PANDA_USDC_DECIMALS").and_then(|s| s.parse().ok()),
            rpc_url: get("PANDA_RPC_URL"),
        }
    }
}

impl Settle {
    /// Resolve from the environment. `db_treasury` is the address the cafe row
    /// carries; `PANDA_TREASURY` wins over it.
    pub fn from_env(db_treasury: &str) -> Self {
        Self::resolve(&Config::from_env(), db_treasury)
    }

    /// The whole decision, with no globals in it.
    pub fn resolve(cfg: &Config, db_treasury: &str) -> Self {
        let chain = cfg
            .chain_key
            .as_deref()
            .and_then(by_key)
            .unwrap_or(CRONOS_TESTNET);
        let decimals = cfg.decimals.unwrap_or(USDC_DECIMALS);

        let token_text = cfg
            .token
            .clone()
            .or_else(|| chain.usdc.map(|s| s.to_string()));
        let treasury_text = cfg
            .treasury
            .clone()
            .unwrap_or_else(|| db_treasury.trim().to_string());

        let token = token_text.as_deref().and_then(parse_address);
        // The seeded 0xC0FFEE… vanity address belongs to nobody. Paying it
        // would burn real USDC, so it never counts as configured.
        let placeholder = treasury_text.eq_ignore_ascii_case(SEED_TREASURY);
        let treasury = if placeholder {
            None
        } else {
            parse_address(&treasury_text)
        };

        let reason = if decimals < USDC_DECIMALS {
            // Prices are held in 6-decimal micro-units; a coarser token cannot
            // represent them.
            format!(
                "PANDA_USDC_DECIMALS={decimals} is below the {USDC_DECIMALS} the menu is priced in"
            )
        } else {
            match (&token_text, token, placeholder, treasury) {
                (None, ..) => format!(
                    "no USDC contract for {}: set PANDA_USDC_ADDRESS",
                    chain.name
                ),
                (Some(t), None, ..) => format!("PANDA_USDC_ADDRESS is not an address: {t}"),
                (_, _, true, _) => {
                    "treasury is still the demo placeholder: set PANDA_TREASURY".into()
                }
                (_, _, _, None) => format!("PANDA_TREASURY is not an address: {treasury_text}"),
                _ => String::new(),
            }
        };
        let usable = reason.is_empty();

        Self {
            chain,
            rpc_url: cfg
                .rpc_url
                .clone()
                .unwrap_or_else(|| chain.rpc_url.to_string()),
            token: if usable { token } else { None },
            token_display: token_text.unwrap_or_default(),
            decimals,
            treasury: if usable { treasury } else { None },
            treasury_display: treasury_text,
            reason,
        }
    }

    /// Play money only, for tests and for a shop with nothing configured.
    pub fn demo() -> Self {
        Self::off(CRONOS_TESTNET, "on-chain settlement is not configured")
    }

    fn off(chain: Chain, reason: &str) -> Self {
        Self {
            chain,
            rpc_url: chain.rpc_url.to_string(),
            token: None,
            token_display: String::new(),
            decimals: USDC_DECIMALS,
            treasury: None,
            treasury_display: String::new(),
            reason: reason.into(),
        }
    }

    /// Both a real token and a real payee are set.
    pub fn onchain(&self) -> bool {
        self.token.is_some() && self.treasury.is_some()
    }

    pub fn token_address(&self) -> &str {
        &self.token_display
    }

    pub fn treasury_address(&self) -> &str {
        &self.treasury_display
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Where receipts are read from.
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Micro-USDC scaled to the token's own atomic units.
    pub fn atomic(&self, micro: i64) -> u128 {
        let micro = micro.max(0) as u128;
        let extra = self.decimals.saturating_sub(USDC_DECIMALS) as u32;
        micro.saturating_mul(10u128.saturating_pow(extra))
    }

    /// `transfer(treasury, amount)` calldata, empty when not configured.
    pub fn call_data(&self, micro: i64) -> String {
        match self.treasury {
            Some(to) => encode_transfer_hex(&to, self.atomic(micro)),
            None => String::new(),
        }
    }

    pub fn explorer_url(&self, tx_hash: &str) -> String {
        if tx_hash.starts_with("0x") {
            format!("{}{tx_hash}", self.chain.explorer_tx)
        } else {
            String::new()
        }
    }

    pub fn wire(&self) -> WireSettlement {
        let denom = Denom::default();
        WireSettlement {
            // The shop decides the mode and how money reads; this is the
            // chain half of the picture, with plain defaults over the rest.
            mode: Mode::Simulation,
            coin_name: COIN_NAME.into(),
            onchain: self.onchain(),
            chain_key: self.chain.key.into(),
            chain_name: self.chain.name.into(),
            chain_id: self.chain.chain_id,
            chain_id_hex: chain_id_hex(self.chain.chain_id),
            // The guest's wallet is pointed at the chain's public node. The
            // shop's own PANDA_RPC_URL, which may be private or carry a key,
            // is for reading receipts and never leaves the server.
            rpc_url: self.chain.rpc_url.into(),
            explorer_tx: self.chain.explorer_tx.into(),
            native_symbol: self.chain.native_symbol.into(),
            native_decimals: self.chain.native_decimals,
            usdc_address: if self.onchain() {
                self.token_display.clone()
            } else {
                String::new()
            },
            usdc_decimals: self.decimals,
            reason: self.reason.clone(),
            denom: DenomView {
                code: denom.code.clone(),
                symbol: denom.symbol.clone(),
                decimals: denom.decimals,
                rate: String::new(),
            },
            faucet_display: String::new(),
            faucet_cap_display: String::new(),
        }
    }
}

/// A transaction hash a browser wallet handed back: 0x + 64 hex.
pub fn is_tx_hash(s: &str) -> bool {
    let t = s.trim();
    t.len() == 66
        && (t.starts_with("0x") || t.starts_with("0X"))
        && t[2..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use causewaybay_panda_protocol::CRONOS_MAINNET;

    const REAL: &str = "0x1111111111111111111111111111111111111111";

    fn cfg(
        chain: &str,
        treasury: Option<&str>,
        token: Option<&str>,
        decimals: Option<u8>,
    ) -> Config {
        Config {
            chain_key: Some(chain.into()),
            treasury: treasury.map(Into::into),
            token: token.map(Into::into),
            decimals,
            rpc_url: None,
        }
    }

    #[test]
    fn a_private_rpc_is_used_for_receipts_but_never_handed_to_a_wallet() {
        let mut own = cfg("cronos_mainnet", Some(REAL), None, None);
        own.rpc_url = Some("http://user:secret@10.0.0.5:8545".into());
        let s = Settle::resolve(&own, SEED_TREASURY);
        assert_eq!(s.rpc_url(), "http://user:secret@10.0.0.5:8545");
        assert_eq!(s.wire().rpc_url, "https://evm.cronos.org");
    }

    #[test]
    fn the_rpc_defaults_to_the_chains_public_node_unless_told_otherwise() {
        let s = Settle::resolve(
            &cfg("cronos_mainnet", Some(REAL), None, None),
            SEED_TREASURY,
        );
        assert_eq!(s.rpc_url(), "https://evm.cronos.org");
        let mut own = cfg("cronos_mainnet", Some(REAL), None, None);
        own.rpc_url = Some("http://127.0.0.1:8545".into());
        assert_eq!(
            Settle::resolve(&own, SEED_TREASURY).rpc_url(),
            "http://127.0.0.1:8545"
        );
    }

    #[test]
    fn nothing_configured_stays_on_play_money() {
        let s = Settle::demo();
        assert!(!s.onchain());
        assert_eq!(s.call_data(4_800_000), "");
        assert_eq!(s.wire().usdc_address, "");
    }

    #[test]
    fn the_environment_is_read_into_the_same_decision() {
        // from_env is only a reader; resolve is the decision the rest tests.
        let s = Settle::from_env(SEED_TREASURY);
        assert_eq!(
            s.chain.key,
            Settle::resolve(&Config::from_env(), SEED_TREASURY)
                .chain
                .key
        );
    }

    // The seeded vanity address is nobody's wallet; sending real USDC there
    // would destroy it.
    #[test]
    fn the_placeholder_treasury_never_enables_onchain() {
        let s = Settle::resolve(&cfg("cronos_mainnet", None, None, None), SEED_TREASURY);
        assert!(!s.onchain());
        assert!(s.reason().contains("placeholder"), "{}", s.reason());
        assert_eq!(s.call_data(4_800_000), "");
    }

    #[test]
    fn mainnet_brings_its_own_usdc_and_only_needs_a_treasury() {
        let s = Settle::resolve(
            &cfg("cronos_mainnet", Some(REAL), None, None),
            SEED_TREASURY,
        );
        assert!(s.onchain(), "{}", s.reason());
        assert_eq!(s.chain.chain_id, 25);
        assert_eq!(s.token_address(), CRONOS_MAINNET.usdc.unwrap());
        assert_eq!(s.wire().chain_id_hex, "0x19");
        // 4.80 USDC at 6 decimals.
        let data = s.call_data(4_800_000);
        assert!(data.starts_with("0xa9059cbb"));
        assert_eq!(data.len(), 2 + 8 + 64 + 64);
        // The payee is in the calldata, so a guest can check it.
        assert!(data.contains(&REAL[2..].to_lowercase()));
    }

    #[test]
    fn testnet_has_no_usdc_until_one_is_named() {
        let bare = Settle::resolve(
            &cfg("cronos_testnet", Some(REAL), None, None),
            SEED_TREASURY,
        );
        assert!(!bare.onchain());
        assert!(
            bare.reason().contains("PANDA_USDC_ADDRESS"),
            "{}",
            bare.reason()
        );

        let named = Settle::resolve(
            &cfg("cronos_testnet", Some(REAL), Some(REAL), None),
            SEED_TREASURY,
        );
        assert!(named.onchain(), "{}", named.reason());
        assert_eq!(named.chain.chain_id, 338);
        assert_eq!(named.wire().chain_id_hex, "0x152");
    }

    #[test]
    fn the_db_treasury_is_used_when_the_environment_names_none() {
        let s = Settle::resolve(&cfg("cronos_mainnet", None, None, None), REAL);
        assert!(s.onchain(), "{}", s.reason());
        assert_eq!(s.treasury_address(), REAL);
    }

    #[test]
    fn a_token_with_more_decimals_scales_the_amount() {
        let s = Settle::resolve(
            &cfg("cronos_mainnet", Some(REAL), Some(REAL), Some(18)),
            SEED_TREASURY,
        );
        assert!(s.onchain(), "{}", s.reason());
        // 4.80 in 18-decimal units, not 6.
        assert_eq!(s.atomic(4_800_000), 4_800_000_000_000_000_000u128);
    }

    #[test]
    fn a_coarser_token_is_refused_rather_than_rounded() {
        let s = Settle::resolve(
            &cfg("cronos_mainnet", Some(REAL), Some(REAL), Some(2)),
            SEED_TREASURY,
        );
        assert!(!s.onchain());
        assert!(s.reason().contains("below"), "{}", s.reason());
    }

    #[test]
    fn a_malformed_address_is_refused_and_says_so() {
        let bad_treasury = Settle::resolve(
            &cfg("cronos_mainnet", Some("not-an-address"), None, None),
            SEED_TREASURY,
        );
        assert!(!bad_treasury.onchain());
        assert!(
            bad_treasury.reason().contains("not an address"),
            "{}",
            bad_treasury.reason()
        );

        let bad_token = Settle::resolve(
            &cfg("cronos_mainnet", Some(REAL), Some("0xnope"), None),
            SEED_TREASURY,
        );
        assert!(!bad_token.onchain());
        assert!(
            bad_token.reason().contains("PANDA_USDC_ADDRESS"),
            "{}",
            bad_token.reason()
        );
    }

    #[test]
    fn an_unknown_chain_falls_back_to_testnet() {
        let s = Settle::resolve(
            &cfg("ethereum", Some(REAL), Some(REAL), None),
            SEED_TREASURY,
        );
        assert_eq!(s.chain.chain_id, 338);
    }

    #[test]
    fn only_a_real_hash_gets_an_explorer_link() {
        let s = Settle::demo();
        assert_eq!(s.explorer_url("demo-0c3d7e86"), "");
        let hash = format!("0x{}", "a".repeat(64));
        assert!(s
            .explorer_url(&hash)
            .starts_with("https://testnet.cronoscan.com/tx/"));
    }

    #[test]
    fn tx_hashes_are_checked_before_they_are_stored() {
        assert!(is_tx_hash(&format!("0x{}", "0123456789abcdef".repeat(4))));
        assert!(!is_tx_hash("demo-0c3d7e86"));
        assert!(!is_tx_hash("0x1234"));
        assert!(!is_tx_hash(&format!("0x{}", "z".repeat(64))));
    }
}
