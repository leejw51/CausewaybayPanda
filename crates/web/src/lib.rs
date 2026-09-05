//! The cafe in a tab. A page talks to this exactly as it talks to the
//! server — `ClientMsg` JSON in, `ServerMsg` JSON out — so the same page
//! runs against either without knowing which. Several "connections" can
//! share one engine (a guest tab and the counter in one demo); frames are
//! routed to them the way the socket hub routes to sockets.

use std::collections::HashMap;

use causewaybay_panda_core::cafe::{self, Apply};
use causewaybay_panda_core::demo::Demo;
use causewaybay_panda_core::{MemStore, SessionRow, Shop, Store};
use causewaybay_panda_protocol::intent::Intent;
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
            } => match cafe::login(&self.store, role, &name, &pin, &session) {
                Ok(row) => {
                    for m in cafe::on_login(&self.store, &self.shop, &row, "", self.demo.on) {
                        out.push(Out { conn, msg: m });
                    }
                    self.conns.insert(conn, Some(row));
                }
                Err(message) => out.push(Out {
                    conn,
                    msg: ServerMsg::Error { message },
                }),
            },
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

    /// The whole shop as JSON, for `localStorage`.
    pub fn snapshot(&self) -> String {
        self.store.snapshot()
    }
}

impl Engine {
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
