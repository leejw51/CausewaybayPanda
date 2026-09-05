//! WebSocket JSON frames. One object, one `"type"`, UTF-8. Unknown types ignored.

use serde::{Deserialize, Serialize};

use crate::money::format_usdc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Guest,
    Owner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayMethod {
    /// Causewaybay Coin: the shop's own test money, in simulation only.
    Coin,
    /// Real USDC, moved from the guest's wallet on Cronos.
    Wallet,
}

/// Which till the shop is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Causewaybay Coin, a faucet, and nothing on a chain. Where a shop
    /// learns the flow, and where the whole suite runs.
    Simulation,
    /// Real USDC on Cronos. No faucet, no test money.
    Live,
}

impl Mode {
    pub fn is_simulation(&self) -> bool {
        matches!(self, Mode::Simulation)
    }
}

/// Where an order has got to. The kitchen moves it along one step at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    /// Paid and waiting to be seen.
    Placed,
    Preparing,
    /// On the counter with the guest's number on it.
    Ready,
    Collected,
    Cancelled,
}

impl OrderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderStatus::Placed => "placed",
            OrderStatus::Preparing => "preparing",
            OrderStatus::Ready => "ready",
            OrderStatus::Collected => "collected",
            OrderStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "placed" | "paid" => Some(OrderStatus::Placed),
            "preparing" | "making" => Some(OrderStatus::Preparing),
            "ready" => Some(OrderStatus::Ready),
            "collected" | "done" => Some(OrderStatus::Collected),
            "cancelled" | "canceled" => Some(OrderStatus::Cancelled),
            _ => None,
        }
    }

    /// The next step the counter would take, if there is one.
    pub fn next(&self) -> Option<Self> {
        match self {
            OrderStatus::Placed => Some(OrderStatus::Preparing),
            OrderStatus::Preparing => Some(OrderStatus::Ready),
            OrderStatus::Ready => Some(OrderStatus::Collected),
            OrderStatus::Collected | OrderStatus::Cancelled => None,
        }
    }

    /// Still the kitchen's problem.
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            OrderStatus::Placed | OrderStatus::Preparing | OrderStatus::Ready
        )
    }

    /// What the guest reads while they wait.
    pub fn guest_line(&self) -> &'static str {
        match self {
            OrderStatus::Placed => "Order received.",
            OrderStatus::Preparing => "The panda is making it.",
            OrderStatus::Ready => "Ready — come and get it.",
            OrderStatus::Collected => "Collected. Enjoy.",
            OrderStatus::Cancelled => "This order was cancelled.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Login {
        role: Role,
        #[serde(default)]
        name: String,
        #[serde(default)]
        pin: String,
        /// A session this browser held before a reload. Honoured when it still
        /// exists with the same role; otherwise a fresh one is opened.
        #[serde(default)]
        session: String,
    },
    Chat {
        text: String,
    },
    Action {
        name: ActionName,
        #[serde(default)]
        item_id: String,
        #[serde(default)]
        qty: u32,
        #[serde(default)]
        method: Option<PayMethod>,
        #[serde(default)]
        item: Option<MenuDraft>,
        #[serde(default)]
        tx_hash: String,
        #[serde(default)]
        order_id: String,
        #[serde(default)]
        status: String,
        /// For `Auto`: on or off.
        #[serde(default)]
        on: bool,
    },
    /// The owner picks who listens to the chat. An empty `key` keeps the one
    /// already held, so a model can be changed without retyping it; provider
    /// "off" hands the chat back to the local parser alone.
    AiSetup {
        provider: String,
        #[serde(default)]
        key: String,
        #[serde(default)]
        model: String,
    },
    Ping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionName {
    Add,
    Remove,
    /// Put an exact count on a line, for the cart's +/- buttons.
    SetQty,
    Cart,
    Pay,
    Clear,
    Menu,
    Help,
    /// Top up the test money. Simulation only.
    Faucet,
    MenuUpsert,
    MenuHide,
    MenuShow,
    ListPayments,
    ListOrders,
    /// Move one order to its next step, or to a named one.
    OrderAdvance,
    OrderCancel,
    /// Let the cafe run itself: simulated guests order and pay, the kitchen
    /// works the tickets. Simulation only.
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuDraft {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_zh: String,
    #[serde(default)]
    pub description: String,
    /// USDC decimal string, e.g. "4.80"
    pub price: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        role: Role,
        name: String,
        /// Keep this and send it back to pick the same session up again.
        session_id: String,
        cafe: String,
        cafe_zh: String,
        treasury: String,
        chain_id: u64,
        balance_usdc: String,
        balance_display: String,
        /// Which model is listening, e.g. "grok · grok-4-fast". Empty for none.
        ai: String,
        settlement: Settlement,
        /// The guest's own open orders, so a reload does not lose them.
        orders: Vec<OrderView>,
    },
    Menu {
        items: Vec<MenuItem>,
    },
    Cart {
        lines: Vec<CartLine>,
        total_usdc: String,
        total_display: String,
        balance_usdc: String,
        balance_display: String,
        can_faucet: bool,
    },
    Assistant {
        text: String,
        buttons: Vec<BigButton>,
    },
    Orders {
        orders: Vec<OrderView>,
    },
    Payments {
        payments: Vec<PaymentView>,
    },
    /// One order changed. The guest who placed it and every owner sees this.
    OrderUpdate {
        order: OrderView,
    },
    /// Whether the cafe is running itself right now.
    Auto {
        on: bool,
    },
    /// Who is listening to the chat. Owners only. The key is never sent back.
    AiStatus {
        /// "grok", "openai", … or "off".
        provider: String,
        model: String,
        /// True when a provider is chosen and it has what it needs.
        ready: bool,
        /// The choices the owner can make, for the setup form.
        providers: Vec<ProviderInfo>,
    },
    /// What the shop has taken today. Owners only; refreshed on every payment.
    Takings {
        total_display: String,
        total_usdc: String,
        orders: i64,
        coin_display: String,
        wallet_display: String,
    },
    Paid {
        order_id: String,
        order_no: i64,
        amount_usdc: String,
        amount_display: String,
        method: PayMethod,
        tx_hash: String,
        call_data: String,
        /// Where to read the transaction, empty for a play-money payment.
        explorer_url: String,
    },
    /// A wallet payment, prepared but not yet made. The browser signs this and
    /// sends the hash back; nothing is debited or recorded until it does.
    PayRequest {
        amount_usdc: String,
        amount_display: String,
        amount_micro: String,
        /// The USDC contract to call.
        token: String,
        /// Who gets paid, for the guest to check against the calldata.
        treasury: String,
        chain_id: u64,
        chain_id_hex: String,
        call_data: String,
    },
    Error {
        message: String,
    },
    Pong,
}

/// What the cafe can settle in, resolved at boot from the environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    /// Simulation (Causewaybay Coin) or live (real USDC on Cronos).
    pub mode: Mode,
    /// What the test money is called, when there is any.
    pub coin_name: String,
    /// True only when a real token and a real treasury are both configured.
    pub onchain: bool,
    pub chain_key: String,
    pub chain_name: String,
    pub chain_id: u64,
    pub chain_id_hex: String,
    pub rpc_url: String,
    pub explorer_tx: String,
    pub native_symbol: String,
    pub native_decimals: u8,
    /// USDC contract, empty when the cafe is on play money only.
    pub usdc_address: String,
    pub usdc_decimals: u8,
    /// Why on-chain settlement is off, for the owner to read. Empty when on.
    pub reason: String,
    /// How every amount is written for people.
    pub denom: DenomView,
    /// What one tap of the faucet hands over, in the display denomination.
    pub faucet_display: String,
    /// A guest may top up while their balance is under this.
    pub faucet_cap_display: String,
}

/// One model provider the owner may choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub key: String,
    pub label: String,
    pub needs_key: bool,
    pub default_model: String,
    /// Where to get a key, for the form's hint.
    pub hint: String,
}

/// The shop's denomination, as the page needs it. Amounts arrive already
/// formatted; this is for labels and for the owner's price box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenomView {
    pub code: String,
    pub symbol: String,
    pub decimals: u8,
    /// Units per USDC, so a page can show the shop's rate honestly.
    pub rate: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub name: String,
    pub name_zh: String,
    pub description: String,
    pub price_usdc: String,
    pub price_micro: i64,
    /// What the board says, in the shop's denomination.
    pub price_display: String,
    pub category: String,
    pub image: String,
    pub available: bool,
}

impl MenuItem {
    pub fn price_label(&self) -> String {
        format!("{} USDC", self.price_usdc)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CartLine {
    pub item_id: String,
    pub name: String,
    pub qty: u32,
    pub unit_usdc: String,
    pub line_usdc: String,
    /// The same two figures as the guest reads them, e.g. "HK$38.00".
    pub unit_display: String,
    pub line_display: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BigButton {
    pub label: String,
    pub action: ActionName,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub qty: u32,
}

impl BigButton {
    pub fn add(item_id: &str, label: &str) -> Self {
        Self {
            label: label.to_string(),
            action: ActionName::Add,
            item_id: item_id.to_string(),
            qty: 1,
        }
    }

    pub fn pay() -> Self {
        Self {
            label: "Pay".into(),
            action: ActionName::Pay,
            item_id: String::new(),
            qty: 0,
        }
    }

    pub fn faucet() -> Self {
        Self {
            label: "Top up".into(),
            action: ActionName::Faucet,
            item_id: String::new(),
            qty: 0,
        }
    }

    pub fn menu() -> Self {
        Self {
            label: "Menu".into(),
            action: ActionName::Menu,
            item_id: String::new(),
            qty: 0,
        }
    }

    pub fn cart() -> Self {
        Self {
            label: "Cart".into(),
            action: ActionName::Cart,
            item_id: String::new(),
            qty: 0,
        }
    }

    pub fn help() -> Self {
        Self {
            label: "Help".into(),
            action: ActionName::Help,
            item_id: String::new(),
            qty: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderView {
    pub id: String,
    /// The number called across the counter: 1, 2, 3 …
    pub order_no: i64,
    pub guest: String,
    pub total_usdc: String,
    pub total_display: String,
    pub status: OrderStatus,
    pub created_at: String,
    pub lines: Vec<CartLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentView {
    pub id: String,
    pub order_id: String,
    /// The amount as the shop's board writes it.
    pub amount_display: String,
    pub guest: String,
    pub amount_usdc: String,
    pub method: String,
    pub tx_hash: String,
    pub status: String,
    pub created_at: String,
}

pub fn usdc_field(micro: i64) -> String {
    format_usdc(micro)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CAFE_NAME;

    #[test]
    fn client_frames_roundtrip() {
        let login = ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        };
        let raw = serde_json::to_string(&login).unwrap();
        assert!(raw.contains("\"type\":\"login\""));
        assert!(raw.contains("\"role\":\"guest\""));
        let back: ClientMsg = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, login);

        let ping: ClientMsg = serde_json::from_str("{\"type\":\"ping\"}").unwrap();
        assert_eq!(ping, ClientMsg::Ping);

        let chat: ClientMsg =
            serde_json::from_str("{\"type\":\"chat\",\"text\":\"latte\"}").unwrap();
        assert!(matches!(chat, ClientMsg::Chat { text } if text == "latte"));
    }

    #[test]
    fn welcome_names_the_cafe() {
        let msg = ServerMsg::Welcome {
            role: Role::Guest,
            name: "Mei".into(),
            session_id: "s-1".into(),
            cafe: CAFE_NAME.into(),
            cafe_zh: crate::CAFE_NAME_ZH.into(),
            treasury: crate::seed::TREASURY.into(),
            chain_id: crate::CHAIN_ID,
            balance_usdc: "50".into(),
            balance_display: "HK$390.00".into(),
            ai: String::new(),
            orders: Vec::new(),
            settlement: Settlement {
                mode: Mode::Live,
                coin_name: crate::COIN_NAME.into(),
                onchain: true,
                chain_key: crate::CRONOS_MAINNET.key.into(),
                chain_name: crate::CRONOS_MAINNET.name.into(),
                chain_id: crate::CRONOS_MAINNET.chain_id,
                chain_id_hex: "0x19".into(),
                rpc_url: crate::CRONOS_MAINNET.rpc_url.into(),
                explorer_tx: crate::CRONOS_MAINNET.explorer_tx.into(),
                native_symbol: "CRO".into(),
                native_decimals: 18,
                usdc_address: crate::CRONOS_MAINNET.usdc.unwrap().into(),
                usdc_decimals: 6,
                reason: String::new(),
                denom: DenomView {
                    code: "HKD".into(),
                    symbol: "HK$".into(),
                    decimals: 2,
                    rate: "7.8".into(),
                },
                faucet_display: "HK$390.00".into(),
                faucet_cap_display: "HK$1,560.00".into(),
            },
        };
        let raw = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, msg);
    }
}
