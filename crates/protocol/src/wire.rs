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
    Usdc,
    Wallet,
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
    },
    Ping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionName {
    Add,
    Remove,
    Cart,
    Pay,
    Clear,
    Menu,
    Help,
    MenuUpsert,
    MenuHide,
    MenuShow,
    ListPayments,
    ListOrders,
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
        cafe: String,
        cafe_zh: String,
        treasury: String,
        chain_id: u64,
        balance_usdc: String,
        grok: bool,
        settlement: Settlement,
    },
    Menu {
        items: Vec<MenuItem>,
    },
    Cart {
        lines: Vec<CartLine>,
        total_usdc: String,
        balance_usdc: String,
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
    Paid {
        order_id: String,
        amount_usdc: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub name: String,
    pub name_zh: String,
    pub description: String,
    pub price_usdc: String,
    pub price_micro: i64,
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
            label: "Pay USDC".into(),
            action: ActionName::Pay,
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
    pub guest: String,
    pub total_usdc: String,
    pub status: String,
    pub created_at: String,
    pub lines: Vec<CartLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentView {
    pub id: String,
    pub order_id: String,
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
            cafe: CAFE_NAME.into(),
            cafe_zh: crate::CAFE_NAME_ZH.into(),
            treasury: crate::seed::TREASURY.into(),
            chain_id: crate::CHAIN_ID,
            balance_usdc: "50".into(),
            grok: false,
            settlement: Settlement {
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
            },
        };
        let raw = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, msg);
    }
}
