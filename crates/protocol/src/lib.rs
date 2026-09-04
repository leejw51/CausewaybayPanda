//! Shared types for Causewaybay Panda: money, menu, wire JSON, chat intents,
//! and the ERC-20 transfer bytes Causewaybay Wallet uses for USDC.

pub mod chain;
pub mod denom;
pub mod erc20;
pub mod intent;
pub mod money;
pub mod seed;
pub mod wire;

pub use chain::{Chain, CRONOS_MAINNET, CRONOS_TESTNET};
pub use denom::Denom;
pub use erc20::encode_transfer;
pub use intent::{parse_all_adds, parse_intent, Intent};
pub use money::{format_usdc, parse_usdc, MICRO};
pub use wire::{ActionName, ClientMsg, Mode, OrderStatus, PayMethod, Role, ServerMsg, Settlement};

pub const CAFE_NAME: &str = "Causewaybay Coffee";
pub const CAFE_NAME_ZH: &str = "銅鑼灣咖啡";
pub const DEFAULT_PIN: &str = "panda";
/// The shop's own test money, spent in simulation and worth nothing anywhere.
pub const COIN_NAME: &str = "Causewaybay Coin";
/// What a guest starts a simulation with: 50 USDC, about HK$390.
pub const GUEST_GRANT: i64 = 50 * MICRO;
/// One pull of the faucet.
pub const FAUCET_GRANT: i64 = 50 * MICRO;
/// A guest may top up while they hold less than this, so the faucet cannot be
/// milked into a silly number.
pub const FAUCET_CAP: i64 = 200 * MICRO;
/// Cronos testnet — the default, and the no-cost chain the wallet already
/// speaks. Override with `PANDA_CHAIN=cronos_mainnet`.
pub const CHAIN_ID: u64 = chain::CRONOS_TESTNET.chain_id;
pub const USDC_DECIMALS: u8 = 6;
