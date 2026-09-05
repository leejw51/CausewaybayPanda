//! The cafe in a tab. A page talks to this exactly as it talks to the
//! server — `ClientMsg` JSON in, `ServerMsg` JSON out — so the same page
//! runs against either without knowing which. Several "connections" can
//! share one engine (a guest tab and the counter in one demo); frames are
//! routed to them the way the socket hub routes to sockets.

use std::collections::HashMap;

use causewaybay_panda_ai::{Ai, Provider, SETTING_KEY, SETTING_MODEL, SETTING_PROVIDER};
use causewaybay_panda_core::cafe::{self, Apply};
use causewaybay_panda_core::demo::Demo;
use causewaybay_panda_core::{MemStore, SessionRow, Shop, Store};
use causewaybay_panda_protocol::intent::{parse_intent, Intent};
use causewaybay_panda_protocol::wire::{ClientMsg, Role, ServerMsg};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// One frame, addressed to one connection.
#[derive(Serialize)]
struct Out {
    conn: u32,
    msg: ServerMsg,
}

#[wasm_bindgen]
pub struct Engine {
    store: MemStore,
    shop: Shop,
    demo: Demo,
    conns: HashMap<u32, Option<SessionRow>>,
    next_conn: u32,
}

#[wasm_bindgen]
impl Engine {
    /// A shop from a saved snapshot, or a fresh one. `pin` is the owner pin
    /// for a fresh shop; `denom` a code like "HKD" or "KRW".
    #[wasm_bindgen(constructor)]
    pub fn new(
        snapshot: Option<String>,
        pin: Option<String>,
        denom: Option<String>,
        seed: f64,
    ) -> Engine {
        let pin = pin.unwrap_or_else(|| "panda".into());
        let mut store = match snapshot.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(s) => MemStore::restore(s, &pin),
            None => MemStore::new(&pin),
        };
        let shop = Shop::resolve(
            &causewaybay_panda_core::shop::Config {
                denom: denom.clone(),
                ..Default::default()
            },
            causewaybay_panda_core::settlement::Settle::demo(),
        );
        store = store.with_denom(shop.denom.clone());
        Engine {
            store,
            shop,
            demo: Demo::new(seed as u64),
            conns: HashMap::new(),
            next_conn: 1,
        }
    }

    /// The host says what time it is. `today` is the local date as a key.
    pub fn set_clock(&mut self, now_ms: f64, today: &str) {
        self.store.set_clock(now_ms as i64, today);
    }

    /// Open a connection, as a browser opening a socket would.
    pub fn connect(&mut self) -> u32 {
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(id, None);
        id
    }

    pub fn disconnect(&mut self, conn: u32) {
        self.conns.remove(&conn);
    }

    /// Handle one client frame. Returns a JSON array of `{conn, msg}`.
    pub fn handle(&mut self, conn: u32, msg_json: &str) -> String {
        let mut out: Vec<Out> = Vec::new();
        let Ok(msg) = serde_json::from_str::<ClientMsg>(msg_json) else {
            return "[]".into();
        };
        match msg {
            ClientMsg::Ping => out.push(Out {
                conn,
                msg: ServerMsg::Pong,
            }),
            ClientMsg::Login {
                role,
                name,
                pin,
                session,
            } => match cafe::login(&self.store, &self.shop, role, &name, &pin, &session) {
                Ok(row) => {
                    let ai = self.ai().map(|a| a.describe()).unwrap_or_default();
                    for m in cafe::on_login(&self.store, &self.shop, &row, &ai, self.demo.on) {
                        out.push(Out { conn, msg: m });
                    }
                    if row.role == Role::Owner {
                        out.push(Out {
                            conn,
                            msg: self.ai_status(),
                        });
                    }
                    self.conns.insert(conn, Some(row));
                }
                Err(message) => out.push(Out {
                    conn,
                    msg: ServerMsg::Error { message },
                }),
            },
            ClientMsg::AiSetup {
                provider,
                key,
                model,
            } => {
                let is_owner =
                    matches!(self.conns.get(&conn), Some(Some(r)) if r.role == Role::Owner);
                if !is_owner {
                    out.push(Out {
                        conn,
                        msg: ServerMsg::Error {
                            message: "only the owner chooses who listens".into(),
                        },
                    });
                } else {
                    match self.set_ai(&provider, &key, &model) {
                        Ok(()) => {
                            let status = self.ai_status();
                            for (c, s) in &self.conns {
                                if matches!(s, Some(r) if r.role == Role::Owner) {
                                    out.push(Out {
                                        conn: *c,
                                        msg: status.clone(),
                                    });
                                }
                            }
                        }
                        Err(message) => out.push(Out {
                            conn,
                            msg: ServerMsg::Error { message },
                        }),
                    }
                }
            }
            other => {
                let Some(Some(row)) = self.conns.get(&conn).cloned() else {
                    out.push(Out {
                        conn,
                        msg: ServerMsg::Error {
                            message: "login first".into(),
                        },
                    });
                    return serde_json::to_string(&out).unwrap_or_else(|_| "[]".into());
                };
                let intents = match &other {
                    ClientMsg::Chat { text } => cafe::intents_for_chat(&self.store, text),
                    _ => Intent::from_client(&other).into_iter().collect(),
                };
                for intent in intents {
                    // The auto switch is the host's. In a one-person demo the
                    // guest may throw it too: there is nobody else to.
                    if let Intent::Auto { on } = intent {
                        match self.demo.set(on, &self.shop) {
                            Ok(now_on) => {
                                out.push(Out {
                                    conn,
                                    msg: ServerMsg::Assistant {
                                        text: if now_on {
                                            "The cafe is running on its own.".into()
                                        } else {
                                            "Stopped.".into()
                                        },
                                        buttons: Vec::new(),
                                    },
                                });
                                for c in self.conns.keys() {
                                    out.push(Out {
                                        conn: *c,
                                        msg: ServerMsg::Auto { on: now_on },
                                    });
                                }
                            }
                            Err(message) => out.push(Out {
                                conn,
                                msg: ServerMsg::Error { message },
                            }),
                        }
                        continue;
                    }
                    let applied = cafe::apply(&self.store, &self.shop, &row, intent);
                    self.route(Some(conn), applied, &mut out);
                }
            }
        }
        serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
    }

    /// One beat of the self-running cafe. Returns routed frames like `handle`.
    pub fn tick(&mut self) -> String {
        let mut out = Vec::new();
        for a in self.demo.tick(&self.store, &self.shop) {
            self.route(None, a, &mut out);
        }
        serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
    }

    pub fn demo_on(&self) -> bool {
        self.demo.on
    }

    /// Can the local parser read this line on its own? When not, and a model
    /// is set up, the host asks it with `ask_ai` and hands the answer to
    /// `apply_intent`.
    pub fn parses(&self, text: &str) -> bool {
        !matches!(
            cafe::intents_for_chat(&self.store, text).first(),
            Some(Intent::Unknown(_))
        )
    }

    /// The owner's choice as JSON `{provider, key, model, board, ready}`, for
    /// the host to call the model with. Empty when nothing is set up.
    pub fn ai_config_json(&self) -> String {
        let Some(ai) = self.ai() else {
            return String::new();
        };
        let board: Vec<String> = self
            .store
            .menu()
            .unwrap_or_default()
            .into_iter()
            .map(|i| {
                let on = if i.available { "on" } else { "off" };
                format!(
                    "{} — {} / {} — {} — {on}",
                    i.id, i.name, i.name_zh, i.price_display
                )
            })
            .collect();
        serde_json::json!({
            "provider": ai.provider.key(),
            "key": self.store.setting(SETTING_KEY).ok().flatten().unwrap_or_default(),
            "model": ai.model,
            "board": board,
        })
        .to_string()
    }

    /// Apply an intent the model produced, as `ask_ai` returned it.
    pub fn apply_intent(&mut self, conn: u32, intent_json: &str) -> String {
        let mut out = Vec::new();
        let Some(Some(row)) = self.conns.get(&conn).cloned() else {
            return "[]".into();
        };
        if let Ok(intent) = serde_json::from_str::<Intent>(intent_json) {
            let applied = cafe::apply(&self.store, &self.shop, &row, intent);
            self.route(Some(conn), applied, &mut out);
        }
        serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
    }

    /// What the parser alone would make of a line — the fallback when the
    /// model has nothing better.
    pub fn local_intent_json(&self, text: &str) -> String {
        serde_json::to_string(&parse_intent(text)).unwrap_or_default()
    }

    /// The whole shop as JSON, for `localStorage`.
    pub fn snapshot(&self) -> String {
        self.store.snapshot()
    }
}

impl Engine {
    fn ai(&self) -> Option<Ai> {
        let get = |k: &str| self.store.setting(k).ok().flatten();
        match get(SETTING_PROVIDER).as_deref() {
            None | Some("off") => None,
            Some(p) => Ai::from_settings(
                Some(p),
                get(SETTING_KEY).as_deref(),
                get(SETTING_MODEL).as_deref(),
            ),
        }
    }

    fn ai_status(&self) -> ServerMsg {
        let (provider, model, ready) = Ai::status(self.ai().as_ref());
        ServerMsg::AiStatus {
            provider,
            model,
            ready,
            providers: Provider::infos(),
        }
    }

    fn set_ai(&mut self, provider: &str, key: &str, model: &str) -> Result<(), String> {
        let provider = provider.trim().to_lowercase();
        if provider == "off" || provider.is_empty() {
            self.store.set_setting(SETTING_PROVIDER, Some("off"))?;
            return Ok(());
        }
        let p =
            Provider::from_key(&provider).ok_or_else(|| format!("no such provider: {provider}"))?;
        let key = if key.trim().is_empty() {
            self.store.setting(SETTING_KEY)?.unwrap_or_default()
        } else {
            key.trim().to_string()
        };
        if p.needs_key() && key.is_empty() {
            return Err(format!("{} needs an API key", p.label()));
        }
        Ai::from_settings(Some(p.key()), Some(&key), Some(model))
            .ok_or_else(|| format!("{} could not be set up", p.label()))?;
        self.store.set_setting(SETTING_PROVIDER, Some(p.key()))?;
        self.store
            .set_setting(SETTING_KEY, if key.is_empty() { None } else { Some(&key) })?;
        self.store.set_setting(
            SETTING_MODEL,
            if model.trim().is_empty() {
                None
            } else {
                Some(model.trim())
            },
        )?;
        Ok(())
    }

    /// The socket hub's fan-out, over connections instead of sockets.
    fn route(&self, from: Option<u32>, a: Apply, out: &mut Vec<Out>) {
        if let Some(c) = from {
            for m in a.to_self {
                out.push(Out { conn: c, msg: m });
            }
        }
        for m in a.to_owners {
            for (c, s) in &self.conns {
                if matches!(s, Some(r) if r.role == Role::Owner) {
                    out.push(Out {
                        conn: *c,
                        msg: m.clone(),
                    });
                }
            }
        }
        for m in a.to_guests {
            for (c, s) in &self.conns {
                if matches!(s, Some(r) if r.role == Role::Guest) {
                    out.push(Out {
                        conn: *c,
                        msg: m.clone(),
                    });
                }
            }
        }
        for (sid, m) in a.to_session {
            for (c, s) in &self.conns {
                if matches!(s, Some(r) if r.id == sid) {
                    out.push(Out {
                        conn: *c,
                        msg: m.clone(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(json: &str) -> Vec<(u32, ServerMsg)> {
        let v: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        v.into_iter()
            .map(|o| {
                (
                    o["conn"].as_u64().unwrap() as u32,
                    serde_json::from_value(o["msg"].clone()).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn a_tab_orders_and_pays_with_no_server_anywhere() {
        let mut e = Engine::new(None, None, None, 7.0);
        e.set_clock(1.0, "2026-09-05");
        let g = e.connect();
        let f = frames(&e.handle(g, r#"{"type":"login","role":"guest","name":"Mei"}"#));
        assert!(matches!(f[0].1, ServerMsg::Welcome { .. }));
        let f = frames(&e.handle(g, r#"{"type":"chat","text":"two lattes"}"#));
        assert!(f.iter().any(|(_, m)| matches!(m, ServerMsg::Cart { total_display, .. } if total_display == "HK$76.00")));
        let f = frames(&e.handle(g, r#"{"type":"chat","text":"pay"}"#));
        assert!(f
            .iter()
            .any(|(_, m)| matches!(m, ServerMsg::Paid { order_no: 1, .. })));
    }

    #[test]
    fn the_counter_in_the_same_engine_sees_the_ticket_land() {
        let mut e = Engine::new(None, None, None, 7.0);
        let o = e.connect();
        e.handle(o, r#"{"type":"login","role":"owner","pin":"panda"}"#);
        let g = e.connect();
        e.handle(g, r#"{"type":"login","role":"guest","name":"Mei"}"#);
        e.handle(g, r#"{"type":"chat","text":"latte"}"#);
        let f = frames(&e.handle(g, r#"{"type":"chat","text":"pay"}"#));
        // The owner's connection is handed the order and the takings.
        assert!(f
            .iter()
            .any(|(c, m)| *c == o && matches!(m, ServerMsg::OrderUpdate { .. })));
        assert!(f
            .iter()
            .any(|(c, m)| *c == o && matches!(m, ServerMsg::Takings { orders: 1, .. })));
        // Another guest hears nothing about it.
        let g2 = e.connect();
        e.handle(g2, r#"{"type":"login","role":"guest","name":"Ling"}"#);
        assert!(!f.iter().any(|(c, _)| *c == g2));
    }

    #[test]
    fn the_cafe_runs_itself_in_a_tab() {
        let mut e = Engine::new(None, None, None, 7.0);
        let o = e.connect();
        e.handle(o, r#"{"type":"login","role":"owner","pin":"panda"}"#);
        let f = frames(&e.handle(o, r#"{"type":"action","name":"auto","on":true}"#));
        assert!(f
            .iter()
            .any(|(_, m)| matches!(m, ServerMsg::Auto { on: true })));
        assert!(e.demo_on());
        let mut tickets = 0;
        for _ in 0..30 {
            tickets += frames(&e.tick())
                .iter()
                .filter(|(c, m)| *c == o && matches!(m, ServerMsg::OrderUpdate { .. }))
                .count();
        }
        assert!(tickets >= 10, "the counter should be busy; saw {tickets}");
        e.handle(o, r#"{"type":"action","name":"auto","on":false}"#);
        assert!(!e.demo_on());
        assert_eq!(e.tick(), "[]");
    }

    #[test]
    fn the_owner_chooses_who_listens_from_the_tab() {
        let mut e = Engine::new(None, None, None, 7.0);
        let o = e.connect();
        let f = frames(&e.handle(o, r#"{"type":"login","role":"owner","pin":"x"}"#));
        assert!(f
            .iter()
            .any(|(_, m)| matches!(m, ServerMsg::AiStatus { ready: false, .. })));
        assert!(e.ai_config_json().is_empty());
        // No key, no listener.
        let f = frames(&e.handle(o, r#"{"type":"ai_setup","provider":"openai"}"#));
        assert!(f
            .iter()
            .any(|(_, m)| matches!(m, ServerMsg::Error { message } if message.contains("key"))));
        // With one, the tab keeps it and the config carries the board.
        let f = frames(&e.handle(
            o,
            r#"{"type":"ai_setup","provider":"openrouter","key":"or-1","model":""}"#,
        ));
        assert!(f.iter().any(|(_, m)| matches!(m, ServerMsg::AiStatus { ready: true, provider, .. } if provider == "openrouter")));
        let cfg: serde_json::Value = serde_json::from_str(&e.ai_config_json()).unwrap();
        assert_eq!(cfg["key"], "or-1");
        assert_eq!(cfg["model"], "openai/gpt-4o-mini");
        assert!(cfg["board"].as_array().unwrap().len() >= 11);
        // The parser still reads what it can; only the rest would go to a model.
        assert!(e.parses("two lattes"));
        assert!(!e.parses("something warm and sweet please"));
        // A guest may not change it.
        let g = e.connect();
        e.handle(g, r#"{"type":"login","role":"guest","name":"Mei"}"#);
        let f = frames(&e.handle(g, r#"{"type":"ai_setup","provider":"off"}"#));
        assert!(f.iter().any(|(_, m)| matches!(m, ServerMsg::Error { .. })));
        // The owner switches it off; the snapshot remembers.
        e.handle(o, r#"{"type":"ai_setup","provider":"off"}"#);
        let e2 = Engine::new(Some(e.snapshot()), None, None, 1.0);
        assert!(e2.ai_config_json().is_empty());
    }

    #[test]
    fn a_model_answer_is_applied_like_a_tap() {
        let mut e = Engine::new(None, None, None, 7.0);
        let g = e.connect();
        e.handle(g, r#"{"type":"login","role":"guest","name":"Mei"}"#);
        let f = frames(&e.apply_intent(g, r#"{"intent":"add","item_id":"latte","qty":2}"#));
        assert!(f.iter().any(|(_, m)| matches!(m, ServerMsg::Cart { total_display, .. } if total_display == "HK$76.00")));
        // Rubbish from a model changes nothing.
        assert_eq!(e.apply_intent(g, "not json"), "[]");
        // And the parser's own reading is there as the fallback.
        assert!(e.local_intent_json("latte").contains("\"add\""));
    }

    #[test]
    fn a_snapshot_reloads_the_same_shop() {
        let mut e = Engine::new(None, None, None, 7.0);
        let g = e.connect();
        let f = frames(&e.handle(g, r#"{"type":"login","role":"guest","name":"Mei"}"#));
        let sid = match &f[0].1 {
            ServerMsg::Welcome { session_id, .. } => session_id.clone(),
            _ => unreachable!(),
        };
        e.handle(g, r#"{"type":"chat","text":"latte"}"#);
        e.handle(g, r#"{"type":"chat","text":"pay"}"#);
        let snap = e.snapshot();

        let mut e2 = Engine::new(Some(snap), None, None, 9.0);
        let g2 = e2.connect();
        let f = frames(&e2.handle(
            g2,
            &format!(r#"{{"type":"login","role":"guest","session":"{sid}"}}"#),
        ));
        match &f[0].1 {
            ServerMsg::Welcome {
                session_id,
                name,
                orders,
                ..
            } => {
                assert_eq!(session_id, &sid);
                assert_eq!(name, "Mei");
                assert_eq!(orders.len(), 1, "the order survives the reload");
            }
            other => panic!("{other:?}"),
        }
    }
}

/// Ask the chosen model to read one line. Returns the intent as JSON, or an
/// empty string when it had nothing better than the parser. Free-standing
/// because an async method cannot borrow the engine across the await.
#[wasm_bindgen]
pub async fn ask_ai(
    provider: String,
    key: String,
    model: String,
    text: String,
    board_json: String,
    role: String,
) -> String {
    let Some(ai) = Ai::from_settings(Some(&provider), Some(&key), Some(&model)) else {
        return String::new();
    };
    let board: Vec<String> = serde_json::from_str(&board_json).unwrap_or_default();
    match ai.interpret(&text, &board, &role).await {
        Some(intent) => serde_json::to_string(&intent).unwrap_or_default(),
        None => String::new(),
    }
}
