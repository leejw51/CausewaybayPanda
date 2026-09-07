//! What the owner may change from the counter, and how it is kept.
//!
//! The environment is the default; the owner's choices are laid over it and
//! kept in the shop's settings, so a restart finds the shop as it was left —
//! the same rule the AI choice follows. A change is checked before it is
//! kept: a shop asked to go live without a treasury stays as it is and is
//! told why, rather than opening a till that cannot bank.

use causewaybay_panda_protocol::chain::{by_key, CHAINS};
use causewaybay_panda_protocol::denom::Denom;
use causewaybay_panda_protocol::erc20::parse_address;
use causewaybay_panda_protocol::wire::{ChainInfo, ServerMsg, Setup};
use causewaybay_panda_protocol::{CAFE_NAME, CAFE_NAME_ZH};

use crate::settlement::{Config as SettleConfig, Settle};
use crate::shop::{Config as ShopConfig, Shop};
use crate::store::Store;

const KEYS: [(&str, fn(&Setup) -> &String, fn(&mut Setup) -> &mut String); 9] = [
    ("cafe.name", |s| &s.name, |s| &mut s.name),
    ("cafe.name_zh", |s| &s.name_zh, |s| &mut s.name_zh),
    ("shop.mode", |s| &s.mode, |s| &mut s.mode),
    ("shop.denom", |s| &s.denom, |s| &mut s.denom),
    ("shop.denom_rate", |s| &s.denom_rate, |s| &mut s.denom_rate),
    ("chain.key", |s| &s.chain, |s| &mut s.chain),
    ("chain.treasury", |s| &s.treasury, |s| &mut s.treasury),
    ("chain.usdc", |s| &s.usdc, |s| &mut s.usdc),
    ("chain.rpc_url", |s| &s.rpc_url, |s| &mut s.rpc_url),
];

/// The owner's choices as the shop keeps them. Empty where none was made.
pub fn from_store(db: &dyn Store) -> Setup {
    let mut s = Setup::default();
    for (key, _, set) in KEYS {
        *set(&mut s) = db.setting(key).ok().flatten().unwrap_or_default();
    }
    s
}

/// Keep the choices. An empty field removes its setting.
pub fn save(db: &dyn Store, s: &Setup) -> Result<(), String> {
    for (key, get, _) in KEYS {
        let v = get(s).trim();
        db.set_setting(key, if v.is_empty() { None } else { Some(v) })?;
    }
    Ok(())
}

/// The environment's defaults with the owner's choices laid over them.
pub fn shop_for(
    s: &Setup,
    env_shop: &ShopConfig,
    env_settle: &SettleConfig,
    db_treasury: &str,
) -> Shop {
    let over = |v: &str, d: &Option<String>| {
        let v = v.trim();
        if v.is_empty() {
            d.clone()
        } else {
            Some(v.to_string())
        }
    };
    let sc = ShopConfig {
        mode: over(&s.mode, &env_shop.mode),
        denom: over(&s.denom, &env_shop.denom),
        rate: over(&s.denom_rate, &env_shop.rate),
        // A symbol or decimals set in the environment belong to that
        // environment's money; a code chosen at the counter uses its own.
        symbol: if s.denom.trim().is_empty() {
            env_shop.symbol.clone()
        } else {
            None
        },
        decimals: if s.denom.trim().is_empty() {
            env_shop.decimals
        } else {
            None
        },
    };
    let st = SettleConfig {
        chain_key: over(&s.chain, &env_settle.chain_key),
        treasury: over(&s.treasury, &env_settle.treasury),
        token: over(&s.usdc, &env_settle.token),
        decimals: env_settle.decimals,
        rpc_url: over(&s.rpc_url, &env_settle.rpc_url),
    };
    Shop::resolve(&sc, Settle::resolve(&st, db_treasury))
}

/// The shop as it should run now: environment, then what the owner kept.
pub fn resolve_shop(db: &dyn Store, env_shop: &ShopConfig, env_settle: &SettleConfig) -> Shop {
    shop_for(
        &from_store(db),
        env_shop,
        env_settle,
        &db.treasury().unwrap_or_default(),
    )
}

/// Check a proposed setup the way a form cannot, then keep it and hand back
/// the shop it makes. Nothing is kept when anything is wrong.
pub fn apply(
    db: &dyn Store,
    proposed: &Setup,
    env_shop: &ShopConfig,
    env_settle: &SettleConfig,
) -> Result<Shop, String> {
    let p = Setup {
        name: proposed.name.trim().to_string(),
        name_zh: proposed.name_zh.trim().to_string(),
        mode: proposed.mode.trim().to_lowercase(),
        denom: proposed.denom.trim().to_uppercase(),
        denom_rate: proposed.denom_rate.trim().to_string(),
        chain: proposed.chain.trim().to_lowercase(),
        treasury: proposed.treasury.trim().to_string(),
        usdc: proposed.usdc.trim().to_string(),
        rpc_url: proposed.rpc_url.trim().to_string(),
    };
    if !matches!(p.mode.as_str(), "" | "simulation" | "live") {
        return Err(format!("mode is simulation or live, not {}", p.mode));
    }
    if !p.denom.is_empty() && Denom::preset(&p.denom).is_none() && p.denom_rate.is_empty() {
        return Err(format!(
            "{} is not a built-in money: give its rate in units per USDC",
            p.denom
        ));
    }
    if !p.denom_rate.is_empty()
        && !p
            .denom_rate
            .parse::<f64>()
            .map(|r| r > 0.0)
            .unwrap_or(false)
    {
        return Err(format!("{} is not a rate", p.denom_rate));
    }
    if !p.chain.is_empty() && by_key(&p.chain).is_none() {
        return Err(format!("no such chain: {}", p.chain));
    }
    if !p.treasury.is_empty() && parse_address(&p.treasury).is_none() {
        return Err("the treasury is not an address".into());
    }
    if !p.usdc.is_empty() && parse_address(&p.usdc).is_none() {
        return Err("the USDC contract is not an address".into());
    }
    if !p.rpc_url.is_empty()
        && !(p.rpc_url.starts_with("http://") || p.rpc_url.starts_with("https://"))
    {
        return Err("the RPC URL must start with http:// or https://".into());
    }
    let shop = shop_for(&p, env_shop, env_settle, &db.treasury().unwrap_or_default());
    if p.mode == "live" && !shop.onchain() {
        // The treasury is the first thing an owner has to name; say so
        // before whatever else the chain needs.
        let t = shop.settle.treasury_address();
        let treasury_missing =
            t.is_empty() || t.eq_ignore_ascii_case(causewaybay_panda_protocol::seed::TREASURY);
        let reason = shop.settle.reason().to_string();
        let why = if treasury_missing && !reason.contains("treasury") {
            format!("name the treasury wallet; also, {reason}")
        } else {
            reason
        };
        return Err(format!("cannot go live: {why}"));
    }
    save(db, &p)?;
    Ok(shop)
}

/// What the door and the welcome call the cafe.
pub fn cafe_name(db: &dyn Store) -> (String, String) {
    let get = |k: &str, d: &str| {
        db.setting(k)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| d.to_string())
    };
    (
        get("cafe.name", CAFE_NAME),
        get("cafe.name_zh", CAFE_NAME_ZH),
    )
}

/// The owner's form: what is kept, what may be chosen, and what it comes to.
pub fn setup_msg(db: &dyn Store, shop: &Shop) -> ServerMsg {
    let (cafe, cafe_zh) = cafe_name(db);
    ServerMsg::Setup {
        setup: from_store(db),
        denoms: Denom::codes().into_iter().map(String::from).collect(),
        chains: CHAINS
            .iter()
            .map(|c| ChainInfo {
                key: c.key.into(),
                name: c.name.into(),
                has_usdc: c.usdc.is_some(),
            })
            .collect(),
        settlement: shop.wire(),
        cafe,
        cafe_zh,
        live_reason: shop.mode_reason.clone(),
    }
}

/// The frame everyone gets when the shop changes.
pub fn shop_msg(db: &dyn Store, shop: &Shop) -> ServerMsg {
    let (cafe, cafe_zh) = cafe_name(db);
    ServerMsg::Shop {
        cafe,
        cafe_zh,
        settlement: shop.wire(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemStore;

    const TREASURY: &str = "0x1111111111111111111111111111111111111111";

    fn env() -> (ShopConfig, SettleConfig) {
        (ShopConfig::default(), SettleConfig::default())
    }

    #[test]
    fn nothing_kept_is_the_environment() {
        let db = MemStore::new("panda");
        let (es, et) = env();
        let shop = resolve_shop(&db, &es, &et);
        assert!(shop.is_simulation());
        assert_eq!(shop.denom.code, "HKD");
        assert_eq!(cafe_name(&db).0, CAFE_NAME);
    }

    #[test]
    fn the_owners_choices_lie_over_the_environment_and_survive() {
        let db = MemStore::new("panda");
        let (es, et) = env();
        let es = ShopConfig {
            denom: Some("KRW".into()),
            ..es
        };
        let shop = apply(
            &db,
            &Setup {
                name: "Panda Corner".into(),
                denom: "jpy".into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap();
        assert_eq!(
            shop.denom.code, "JPY",
            "the counter wins over the environment"
        );
        assert_eq!(
            cafe_name(&db),
            ("Panda Corner".to_string(), CAFE_NAME_ZH.to_string())
        );
        // A fresh resolve from the same store finds it again.
        let again = resolve_shop(&db, &es, &et);
        assert_eq!(again.denom.code, "JPY");
        // Clearing the code falls back to the environment's KRW.
        let shop = apply(
            &db,
            &Setup {
                name: "Panda Corner".into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap();
        assert_eq!(shop.denom.code, "KRW");
    }

    #[test]
    fn a_shop_cannot_go_live_without_a_treasury_and_nothing_is_kept() {
        let db = MemStore::new("panda");
        let (es, et) = env();
        let err = apply(
            &db,
            &Setup {
                mode: "live".into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap_err();
        assert!(err.contains("cannot go live"), "{err}");
        assert!(err.contains("treasury"), "{err}");
        assert_eq!(
            from_store(&db),
            Setup::default(),
            "a refused setup leaves no trace"
        );

        let shop = apply(
            &db,
            &Setup {
                mode: "live".into(),
                chain: "cronos_mainnet".into(),
                treasury: TREASURY.into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap();
        assert!(!shop.is_simulation());
        assert!(shop.onchain());
        assert_eq!(shop.settle.treasury_address(), TREASURY);
        assert_eq!(shop.settle.chain.chain_id, 25);
        // The testnet has no built-in USDC: going there needs the token named.
        let err = apply(
            &db,
            &Setup {
                mode: "live".into(),
                chain: "cronos_testnet".into(),
                treasury: TREASURY.into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap_err();
        assert!(err.contains("USDC"), "{err}");
    }

    #[test]
    fn the_form_is_checked_before_anything_moves() {
        let db = MemStore::new("panda");
        let (es, et) = env();
        let bad = |s: Setup| apply(&db, &s, &es, &et).unwrap_err();
        assert!(bad(Setup {
            mode: "maybe".into(),
            ..Default::default()
        })
        .contains("mode"));
        assert!(bad(Setup {
            denom: "PND".into(),
            ..Default::default()
        })
        .contains("rate"));
        assert!(bad(Setup {
            denom: "PND".into(),
            denom_rate: "lots".into(),
            ..Default::default()
        })
        .contains("rate"));
        assert!(bad(Setup {
            chain: "ethereum".into(),
            ..Default::default()
        })
        .contains("chain"));
        assert!(bad(Setup {
            treasury: "0x12".into(),
            ..Default::default()
        })
        .contains("address"));
        assert!(bad(Setup {
            usdc: "usdc".into(),
            ..Default::default()
        })
        .contains("address"));
        assert!(bad(Setup {
            rpc_url: "evm.cronos.org".into(),
            ..Default::default()
        })
        .contains("http"));
        // A made-up money with a rate is fine: the owner knows their peg.
        let shop = apply(
            &db,
            &Setup {
                denom: "PND".into(),
                denom_rate: "2.5".into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap();
        assert_eq!(shop.denom.code, "PND");
        assert_eq!(shop.price(1_000_000), "PND2.50");
    }

    #[test]
    fn the_form_frame_carries_the_choices_and_the_result() {
        let db = MemStore::new("panda");
        let (es, et) = env();
        let shop = apply(
            &db,
            &Setup {
                denom: "KRW".into(),
                ..Default::default()
            },
            &es,
            &et,
        )
        .unwrap();
        match setup_msg(&db, &shop) {
            ServerMsg::Setup {
                setup,
                denoms,
                chains,
                settlement,
                live_reason,
                ..
            } => {
                assert_eq!(setup.denom, "KRW");
                assert!(denoms.contains(&"KRW".to_string()));
                assert_eq!(chains.len(), 2);
                assert!(chains
                    .iter()
                    .any(|c| c.key == "cronos_mainnet" && c.has_usdc));
                assert_eq!(settlement.denom.code, "KRW");
                assert!(live_reason.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }
}
