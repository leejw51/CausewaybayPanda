//! The Cronos chains the till can settle on, and the USDC contract on each.
//!
//! Addresses and ids match the Causewaybay Wallet network table. Cronos
//! mainnet has a canonical bridged USDC; the testnet has no official one, so a
//! testnet operator must name their own token with `PANDA_USDC_ADDRESS`.

/// One EVM network, with everything a browser wallet needs to be pointed at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chain {
    /// Stable key used by `PANDA_CHAIN`.
    pub key: &'static str,
    pub name: &'static str,
    pub chain_id: u64,
    pub native_symbol: &'static str,
    pub native_decimals: u8,
    pub rpc_url: &'static str,
    /// Prefix a transaction hash is appended to.
    pub explorer_tx: &'static str,
    /// Canonical USDC, when the network has one.
    pub usdc: Option<&'static str>,
}

pub const CRONOS_MAINNET: Chain = Chain {
    key: "cronos_mainnet",
    name: "Cronos Mainnet",
    chain_id: 25,
    native_symbol: "CRO",
    native_decimals: 18,
    rpc_url: "https://evm.cronos.org",
    explorer_tx: "https://cronoscan.com/tx/",
    usdc: Some("0xc21223249CA28397B4B6541dfFaEcC539BfF0c59"),
};

pub const CRONOS_TESTNET: Chain = Chain {
    key: "cronos_testnet",
    name: "Cronos Testnet",
    chain_id: 338,
    native_symbol: "TCRO",
    native_decimals: 18,
    rpc_url: "https://evm-t3.cronos.org",
    explorer_tx: "https://testnet.cronoscan.com/tx/",
    // No canonical testnet USDC: the operator names their own token.
    usdc: None,
};

pub const CHAINS: [Chain; 2] = [CRONOS_TESTNET, CRONOS_MAINNET];

/// Look a chain up by key ("cronos_mainnet") or by decimal id ("25").
pub fn by_key(key: &str) -> Option<Chain> {
    let k = key.trim().to_lowercase();
    CHAINS
        .iter()
        .find(|c| c.key == k || c.chain_id.to_string() == k)
        .copied()
}

/// `eth_chainId` / `wallet_switchEthereumChain` form: minimal hex, `0x` prefix.
pub fn chain_id_hex(id: u64) -> String {
    format!("0x{id:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erc20::parse_address;

    #[test]
    fn keys_and_ids_both_resolve() {
        assert_eq!(by_key("cronos_mainnet").unwrap().chain_id, 25);
        assert_eq!(by_key("CRONOS_TESTNET").unwrap().chain_id, 338);
        assert_eq!(by_key("25").unwrap().key, "cronos_mainnet");
        assert_eq!(by_key("338").unwrap().key, "cronos_testnet");
        assert!(by_key("ethereum").is_none());
    }

    #[test]
    fn mainnet_usdc_is_a_real_address_and_testnet_has_none() {
        let usdc = CRONOS_MAINNET.usdc.unwrap();
        assert!(parse_address(usdc).is_some(), "{usdc} must parse");
        assert_eq!(usdc.len(), 42);
        assert!(CRONOS_TESTNET.usdc.is_none());
    }

    #[test]
    fn hex_ids_match_what_a_wallet_expects() {
        assert_eq!(chain_id_hex(25), "0x19");
        assert_eq!(chain_id_hex(338), "0x152");
    }
}
