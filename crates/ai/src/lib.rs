//! The waiter's ear. One provider-agnostic call that turns a sentence into an
//! `Intent`; the local parser always runs first, so this is only ever reached
//! for something it could not read.
//!
//! ```text
//! PANDA_AI_PROVIDER=grok|openai|anthropic|ollama|openrouter|off
//! PANDA_AI_MODEL=…        # override the provider's default
//! PANDA_AI_BASE_URL=…     # a proxy, or a non-default Ollama host
//! ```
//!
//! With nothing set, the first key present wins, in the order above. No key
//! and no Ollama means the local parser is the whole brain, which is a
//! perfectly good cafe.
//!
//! The owner can also choose from the counter at run time; those choices are
//! kept in the shop's settings and win over the environment. The same code
//! runs in a browser tab over `fetch`, where the key stays in that tab.

use causewaybay_panda_protocol::intent::Intent;
use causewaybay_panda_protocol::wire::{MenuDraft, PayMethod, ProviderInfo};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Grok,
    OpenAi,
    Anthropic,
    Ollama,
    OpenRouter,
}

/// Every provider the owner may pick, in the order the form shows them.
pub const ALL: [Provider; 5] = [
    Provider::Grok,
    Provider::OpenAi,
    Provider::Anthropic,
    Provider::OpenRouter,
    Provider::Ollama,
];

/// Settings keys the shop keeps the owner's choice under.
pub const SETTING_PROVIDER: &str = "ai.provider";
pub const SETTING_KEY: &str = "ai.key";
pub const SETTING_MODEL: &str = "ai.model";

impl Provider {
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Grok => "Grok (x.ai)",
            Provider::OpenAi => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Ollama => "Ollama (on this machine)",
            Provider::OpenRouter => "OpenRouter",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            Provider::Grok => "console.x.ai",
            Provider::OpenAi => "platform.openai.com",
            Provider::Anthropic => "console.anthropic.com",
            Provider::Ollama => "no key; runs at localhost:11434",
            Provider::OpenRouter => "openrouter.ai/keys",
        }
    }

    pub fn needs_key(&self) -> bool {
        !matches!(self, Provider::Ollama)
    }

    /// What the owner's form needs to know about each choice.
    pub fn infos() -> Vec<ProviderInfo> {
        ALL.iter()
            .map(|p| ProviderInfo {
                key: p.key().into(),
                label: p.label().into(),
                needs_key: p.needs_key(),
                default_model: p.default_model().into(),
                hint: p.hint().into(),
            })
            .collect()
    }

    pub fn key(&self) -> &'static str {
        match self {
            Provider::Grok => "grok",
            Provider::OpenAi => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Ollama => "ollama",
            Provider::OpenRouter => "openrouter",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "grok" | "xai" | "x.ai" => Some(Provider::Grok),
            "openai" | "gpt" => Some(Provider::OpenAi),
            "anthropic" | "claude" => Some(Provider::Anthropic),
            "ollama" | "local" => Some(Provider::Ollama),
            "openrouter" => Some(Provider::OpenRouter),
            _ => None,
        }
    }

    /// Every provider here speaks OpenAI's chat-completions shape except
    /// Anthropic, whose Messages API keeps the system prompt out of the turns.
    fn is_anthropic(&self) -> bool {
        matches!(self, Provider::Anthropic)
    }

    fn default_base(&self) -> &'static str {
        match self {
            Provider::Grok => "https://api.x.ai/v1",
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com/v1",
            Provider::Ollama => "http://localhost:11434/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::Grok => "grok-4-fast",
            Provider::OpenAi => "gpt-4o-mini",
            Provider::Anthropic => "claude-opus-5",
            Provider::Ollama => "llama3.2",
            Provider::OpenRouter => "openai/gpt-4o-mini",
        }
    }

    /// The environment variables that hold this provider's key, in order.
    fn key_vars(&self) -> &'static [&'static str] {
        match self {
            Provider::Grok => &["XAI_API_KEY", "GROK_API_KEY"],
            Provider::OpenAi => &["OPENAI_API_KEY"],
            Provider::Anthropic => &["ANTHROPIC_API_KEY"],
            Provider::OpenRouter => &["OPENROUTER_API_KEY"],
            // Ollama runs on the same machine and wants no key.
            Provider::Ollama => &[],
        }
    }
}

/// A resolved brain: which provider, which model, and the credential to use.
#[derive(Debug, Clone)]
pub struct Ai {
    pub provider: Provider,
    pub model: String,
    base_url: String,
    api_key: String,
}

/// What the environment said, lifted out so tests never touch process globals.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    /// provider key -> credential, for whatever was found in the environment.
    pub keys: Vec<(String, String)>,
    /// Ollama needs no key, so its presence is signalled separately.
    pub ollama_host: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let get = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        let order = [
            Provider::Grok,
            Provider::OpenAi,
            Provider::Anthropic,
            Provider::OpenRouter,
        ];
        let mut keys = Vec::new();
        for p in order {
            if let Some(v) = p.key_vars().iter().find_map(|k| get(k)) {
                keys.push((p.key().to_string(), v));
            }
        }
        Self {
            provider: get("PANDA_AI_PROVIDER"),
            model: get("PANDA_AI_MODEL"),
            base_url: get("PANDA_AI_BASE_URL"),
            keys,
            ollama_host: get("OLLAMA_HOST"),
        }
    }
}

impl Ai {
    pub fn from_env() -> Option<Self> {
        Self::resolve(&Config::from_env())
    }

    /// The owner's choice from the counter: provider, key, model. "off" or an
    /// unknown provider means the local parser alone.
    pub fn from_settings(
        provider: Option<&str>,
        key: Option<&str>,
        model: Option<&str>,
    ) -> Option<Self> {
        let p = Provider::from_key(provider?)?;
        let cfg = Config {
            provider: Some(p.key().into()),
            model: model
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty()),
            // A proxy or a stand-in set by the operator applies to any provider
            // the owner picks. In a tab there is no environment: none.
            base_url: Config::from_env().base_url,
            keys: key
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .map(|k| vec![(p.key().to_string(), k)])
                .unwrap_or_default(),
            ollama_host: (p == Provider::Ollama).then(|| "http://localhost:11434".to_string()),
        };
        Self::resolve(&cfg)
    }

    /// What the counter shows: never the key.
    pub fn status(ai: Option<&Ai>) -> (String, String, bool) {
        match ai {
            Some(a) => (a.provider.key().into(), a.model.clone(), true),
            None => ("off".into(), String::new(), false),
        }
    }

    /// Pick a provider. An explicit choice is honoured or refused outright;
    /// otherwise the first credential found wins.
    pub fn resolve(cfg: &Config) -> Option<Self> {
        let key_for = |p: Provider| {
            cfg.keys
                .iter()
                .find(|(k, _)| k == p.key())
                .map(|(_, v)| v.clone())
        };

        let chosen = match cfg.provider.as_deref() {
            Some(name) if name.trim().eq_ignore_ascii_case("off") => return None,
            Some(name) => Provider::from_key(name)?,
            None => [
                Provider::Grok,
                Provider::OpenAi,
                Provider::Anthropic,
                Provider::OpenRouter,
            ]
            .into_iter()
            .find(|p| key_for(*p).is_some())
            .or(cfg.ollama_host.as_ref().map(|_| Provider::Ollama))?,
        };

        // Everyone but Ollama must produce a credential, however they were
        // chosen. A named provider with no key is off, not a broken cafe.
        let api_key = match key_for(chosen) {
            Some(k) => k,
            None if chosen == Provider::Ollama => String::new(),
            None => return None,
        };

        let base_url = cfg
            .base_url
            .clone()
            .or_else(|| {
                // OLLAMA_HOST is a host, not a v1 root; make it one.
                (chosen == Provider::Ollama).then(|| {
                    let h = cfg.ollama_host.clone().unwrap_or_default();
                    let h = h.trim_end_matches('/');
                    if h.is_empty() {
                        chosen.default_base().to_string()
                    } else if h.ends_with("/v1") {
                        h.to_string()
                    } else {
                        format!("{h}/v1")
                    }
                })
            })
            .unwrap_or_else(|| chosen.default_base().to_string());

        Some(Self {
            provider: chosen,
            model: cfg
                .model
                .clone()
                .unwrap_or_else(|| chosen.default_model().to_string()),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }

    pub fn describe(&self) -> String {
        format!("{} · {}", self.provider.key(), self.model)
    }

    /// The credential in use — for the host that holds it, never for a page.
    pub fn key(&self) -> &str {
        &self.api_key
    }

    fn endpoint(&self) -> String {
        if self.provider.is_anthropic() {
            format!("{}/messages", self.base_url)
        } else {
            format!("{}/chat/completions", self.base_url)
        }
    }

    fn body(&self, system: &str, user: &str) -> Value {
        if self.provider.is_anthropic() {
            // Messages API: system is its own field, max_tokens is required.
            json!({
                "model": self.model,
                "max_tokens": 512,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            })
        } else {
            json!({
                "model": self.model,
                "temperature": 0,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            })
        }
    }

    /// Pull the assistant's text out of whichever envelope came back.
    pub fn text_from(&self, v: &Value) -> Option<String> {
        if self.provider.is_anthropic() {
            v["content"]
                .as_array()?
                .iter()
                .find(|b| b["type"] == "text")
                .and_then(|b| b["text"].as_str())
                .map(str::to_string)
        } else {
            v["choices"][0]["message"]["content"]
                .as_str()
                .map(str::to_string)
        }
    }

    /// Ask the model to read one line. `None` means "keep the local parser's
    /// answer" — a failure here is never fatal to an order.
    /// `facts` are what the till knows right now — the day's figures for the
    /// owner, their own orders for a guest — so a question can be answered
    /// from the books rather than guessed.
    pub async fn interpret(
        &self,
        text: &str,
        menu: &[String],
        role: &str,
        facts: &[String],
    ) -> Option<Intent> {
        let verbs = if role == "owner" {
            "help, menu, payments, orders, dashboard, add_item, hide, show, say"
        } else {
            "help, menu, cart, add, remove, pay, clear, faucet, dashboard, say"
        };
        let system = format!(
            "You turn Causewaybay Coffee chat into one JSON object, nothing else.\n\
             The speaker is the {role}. Allowed intent values: {verbs}.\n\
             Keys: intent, item_id (an id from the menu below), qty, \
             method (coin|wallet), name, name_zh, price, category, text, suggest.\n\
             For add_item always fill name, price, category, and name_zh with \
             Traditional Chinese for the dish.\n\
             When the line is a question, small talk, or asks for advice or a \
             recommendation, answer as the cafe's panda barista: intent say, \
             text (one or two short sentences, in the speaker's language), and \
             suggest (up to three item_ids from the menu worth adding, else []). \
             Answer from the facts below when they cover the question; never \
             invent figures.\n\
             Facts:\n{}\n\
             Menu:\n{}",
            facts.join("\n"),
            menu.join("\n")
        );

        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(std::time::Duration::from_secs(20));
        let client = builder.build().ok()?;
        let mut req = client.post(self.endpoint()).json(&self.body(&system, text));
        if self.provider.is_anthropic() {
            req = req
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01");
            // Anthropic will answer a browser only when told the key is meant
            // to be there. It is: the tab is the owner's own.
            #[cfg(target_arch = "wasm32")]
            {
                req = req.header("anthropic-dangerous-direct-browser-access", "true");
            }
        } else if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let v: Value = req.send().await.ok()?.json().await.ok()?;
        intent_from_model_json(&self.text_from(&v)?)
    }
}

/// Pull the first JSON object out of a model reply (raw or fenced) and turn it
/// into an intent. `None` means "fall back to the local parser".
pub fn intent_from_model_json(raw: &str) -> Option<Intent> {
    let json = extract_json(raw)?;
    let v: Value = serde_json::from_str(&json).ok()?;
    let intent = v.get("intent")?.as_str()?.to_lowercase();
    match intent.as_str() {
        "help" => Some(Intent::Help),
        "menu" | "show_menu" => Some(Intent::ShowMenu),
        "cart" | "show_cart" => Some(Intent::ShowCart),
        "clear" => Some(Intent::Clear),
        "faucet" | "topup" | "top_up" => Some(Intent::Faucet),
        "pay" => {
            let method = match v.get("method").and_then(|m| m.as_str()).unwrap_or("coin") {
                "wallet" => PayMethod::Wallet,
                _ => PayMethod::Coin,
            };
            Some(Intent::Pay {
                method,
                tx_hash: String::new(),
            })
        }
        "add" => {
            let item_id = v.get("item_id")?.as_str()?.to_string();
            let qty = v.get("qty").and_then(|q| q.as_u64()).unwrap_or(1) as u32;
            Some(Intent::Add {
                item_id,
                qty: qty.max(1),
            })
        }
        "remove" => Some(Intent::Remove {
            item_id: v.get("item_id")?.as_str()?.to_string(),
            qty: v.get("qty").and_then(|q| q.as_u64()).unwrap_or(1) as u32,
        }),
        "payments" | "list_payments" => Some(Intent::ListPayments),
        "orders" | "list_orders" => Some(Intent::ListOrders),
        "dashboard" | "stats" | "today" | "summary" => Some(Intent::Dashboard),
        "say" | "answer" | "reply" | "talk" => {
            let text = v.get("text")?.as_str()?.trim().to_string();
            if text.is_empty() {
                return None;
            }
            let suggest = v
                .get("suggest")
                .and_then(|s| s.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .take(3)
                        .collect()
                })
                .unwrap_or_default();
            Some(Intent::Say { text, suggest })
        }
        "menu_upsert" | "add_item" => {
            let draft = MenuDraft {
                id: v
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: v.get("name")?.as_str()?.to_string(),
                name_zh: v
                    .get("name_zh")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: String::new(),
                price: v.get("price")?.as_str()?.to_string(),
                category: v
                    .get("category")
                    .and_then(|x| x.as_str())
                    .unwrap_or("other")
                    .to_string(),
                image: String::new(),
            };
            Some(Intent::MenuUpsert { draft })
        }
        "menu_hide" | "hide" => Some(Intent::MenuHide {
            item_id: v.get("item_id")?.as_str()?.to_string(),
        }),
        "menu_show" | "show" | "unhide" => Some(Intent::MenuShow {
            item_id: v.get("item_id")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

fn extract_json(raw: &str) -> Option<String> {
    let t = raw.trim();
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    (end >= start).then(|| t[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: Option<&str>, keys: &[(&str, &str)]) -> Config {
        Config {
            provider: provider.map(Into::into),
            keys: keys
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_spoken_answer_is_read_with_its_suggestions() {
        let i = intent_from_model_json(
            r#"{"intent":"say","text":"The milk tea is the one to have.","suggest":["milk_tea","egg_tart",""]}"#,
        )
        .unwrap();
        match i {
            Intent::Say { text, suggest } => {
                assert_eq!(text, "The milk tea is the one to have.");
                assert_eq!(suggest, vec!["milk_tea", "egg_tart"]);
            }
            other => panic!("{other:?}"),
        }
        // Nothing to say is nothing: the parser's answer stands.
        assert!(intent_from_model_json(r#"{"intent":"say","text":"  "}"#).is_none());
        assert!(matches!(
            intent_from_model_json(r#"{"intent":"say","text":"Hi"}"#),
            Some(Intent::Say { suggest, .. }) if suggest.is_empty()
        ));
    }

    #[test]
    fn no_credentials_anywhere_means_the_local_parser_is_the_whole_brain() {
        assert!(Ai::resolve(&Config::default()).is_none());
    }

    #[test]
    fn the_first_key_present_wins_when_nothing_is_named() {
        let a = Ai::resolve(&cfg(None, &[("openai", "sk-1"), ("grok", "xai-1")])).unwrap();
        assert_eq!(a.provider, Provider::Grok, "grok is preferred");
        let b = Ai::resolve(&cfg(None, &[("anthropic", "sk-2"), ("openai", "sk-1")])).unwrap();
        assert_eq!(b.provider, Provider::OpenAi);
        let c = Ai::resolve(&cfg(None, &[("openrouter", "or-1")])).unwrap();
        assert_eq!(c.provider, Provider::OpenRouter);
    }

    #[test]
    fn naming_a_provider_overrides_whatever_else_is_lying_around() {
        let a = Ai::resolve(&cfg(
            Some("anthropic"),
            &[("grok", "xai-1"), ("anthropic", "sk-ant")],
        ))
        .unwrap();
        assert_eq!(a.provider, Provider::Anthropic);
        assert_eq!(a.model, "claude-opus-5");
        assert_eq!(a.endpoint(), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn a_named_provider_with_no_key_is_off_rather_than_broken() {
        assert!(Ai::resolve(&cfg(Some("anthropic"), &[("grok", "xai-1")])).is_none());
        assert!(Ai::resolve(&cfg(Some("nonsense"), &[("grok", "xai-1")])).is_none());
        assert!(Ai::resolve(&cfg(Some("off"), &[("grok", "xai-1")])).is_none());
    }

    #[test]
    fn ollama_needs_no_key_and_its_host_becomes_a_v1_root() {
        let mut c = cfg(Some("ollama"), &[]);
        c.ollama_host = Some("http://192.168.1.9:11434".into());
        let a = Ai::resolve(&c).unwrap();
        assert_eq!(a.provider, Provider::Ollama);
        assert_eq!(a.endpoint(), "http://192.168.1.9:11434/v1/chat/completions");
        assert!(a.api_key.is_empty());

        // A host that already names /v1 is not doubled.
        c.ollama_host = Some("http://localhost:11434/v1/".into());
        assert_eq!(
            Ai::resolve(&c).unwrap().endpoint(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn ollama_is_the_last_resort_when_no_key_exists() {
        let mut c = Config::default();
        assert!(Ai::resolve(&c).is_none());
        c.ollama_host = Some("http://localhost:11434".into());
        assert_eq!(Ai::resolve(&c).unwrap().provider, Provider::Ollama);
    }

    #[test]
    fn the_model_and_base_url_can_both_be_overridden() {
        let mut c = cfg(Some("openai"), &[("openai", "sk-1")]);
        c.model = Some("gpt-5".into());
        c.base_url = Some("https://proxy.internal/v1/".into());
        let a = Ai::resolve(&c).unwrap();
        assert_eq!(a.model, "gpt-5");
        assert_eq!(a.endpoint(), "https://proxy.internal/v1/chat/completions");
        assert_eq!(a.describe(), "openai · gpt-5");
    }

    #[test]
    fn anthropic_puts_the_system_prompt_beside_the_turns_not_in_them() {
        let a = Ai::resolve(&cfg(Some("anthropic"), &[("anthropic", "k")])).unwrap();
        let b = a.body("be brief", "latte");
        assert_eq!(b["system"], "be brief");
        assert_eq!(b["messages"].as_array().unwrap().len(), 1);
        assert_eq!(b["messages"][0]["role"], "user");
        // max_tokens is required by the Messages API.
        assert!(b["max_tokens"].is_number());
    }

    #[test]
    fn the_openai_shape_carries_the_system_prompt_as_a_turn() {
        let a = Ai::resolve(&cfg(Some("grok"), &[("grok", "k")])).unwrap();
        let b = a.body("be brief", "latte");
        assert!(b["system"].is_null());
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(b["temperature"], 0);
    }

    #[test]
    fn each_envelope_gives_up_its_text() {
        let anthropic = Ai::resolve(&cfg(Some("anthropic"), &[("anthropic", "k")])).unwrap();
        let v = json!({"content": [{"type": "text", "text": "{\"intent\":\"menu\"}"}]});
        assert_eq!(anthropic.text_from(&v).unwrap(), "{\"intent\":\"menu\"}");

        let openai = Ai::resolve(&cfg(Some("openai"), &[("openai", "k")])).unwrap();
        let v = json!({"choices": [{"message": {"content": "{\"intent\":\"menu\"}"}}]});
        assert_eq!(openai.text_from(&v).unwrap(), "{\"intent\":\"menu\"}");

        // An error envelope yields nothing rather than a wrong intent.
        assert!(openai
            .text_from(&json!({"error": {"message": "nope"}}))
            .is_none());
        assert!(anthropic.text_from(&json!({"type": "error"})).is_none());
    }

    #[test]
    fn a_choice_from_the_counter_keeps_the_operators_base_url() {
        // The environment is process-wide; set, check, and restore.
        let before = std::env::var("PANDA_AI_BASE_URL").ok();
        std::env::set_var("PANDA_AI_BASE_URL", "http://127.0.0.1:9/v1");
        let a = Ai::from_settings(Some("openrouter"), Some("or-1"), None).unwrap();
        match before {
            Some(v) => std::env::set_var("PANDA_AI_BASE_URL", v),
            None => std::env::remove_var("PANDA_AI_BASE_URL"),
        }
        assert_eq!(a.endpoint(), "http://127.0.0.1:9/v1/chat/completions");
    }

    #[test]
    fn the_owner_can_choose_from_the_counter() {
        let a = Ai::from_settings(Some("anthropic"), Some("sk-ant"), Some("")).unwrap();
        assert_eq!(a.provider, Provider::Anthropic);
        assert_eq!(a.model, "claude-opus-5", "an empty model means the default");
        let b = Ai::from_settings(
            Some("openrouter"),
            Some("or-1"),
            Some("meta-llama/llama-3.3-70b-instruct"),
        )
        .unwrap();
        assert_eq!(b.model, "meta-llama/llama-3.3-70b-instruct");
        // No key, no listener — except Ollama, which needs none.
        assert!(Ai::from_settings(Some("openai"), None, None).is_none());
        assert!(Ai::from_settings(Some("openai"), Some("  "), None).is_none());
        assert!(Ai::from_settings(Some("ollama"), None, None).is_some());
        assert!(Ai::from_settings(Some("off"), Some("k"), None).is_none());
        assert!(Ai::from_settings(None, Some("k"), None).is_none());
        let (p, m, ready) = Ai::status(None);
        assert_eq!((p.as_str(), m.as_str(), ready), ("off", "", false));
        assert_eq!(Provider::infos().len(), 5);
        assert!(Provider::infos().iter().any(|i| i.key == "openrouter"));
    }

    #[test]
    fn parses_fenced_add() {
        let raw = "```json\n{\"intent\":\"add\",\"item_id\":\"latte\",\"qty\":2}\n```";
        match intent_from_model_json(raw) {
            Some(Intent::Add { item_id, qty }) => {
                assert_eq!(item_id, "latte");
                assert_eq!(qty, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_pay_faucet_and_owner_verbs() {
        assert!(matches!(
            intent_from_model_json("{\"intent\":\"pay\",\"method\":\"wallet\"}"),
            Some(Intent::Pay {
                method: PayMethod::Wallet,
                ..
            })
        ));
        assert!(matches!(
            intent_from_model_json("{\"intent\":\"pay\"}"),
            Some(Intent::Pay {
                method: PayMethod::Coin,
                ..
            })
        ));
        assert_eq!(
            intent_from_model_json("{\"intent\":\"faucet\"}"),
            Some(Intent::Faucet)
        );
        match intent_from_model_json(
            "{\"intent\":\"add_item\",\"name\":\"mango pudding\",\"price\":\"3.20\",\"category\":\"dessert\"}",
        ) {
            Some(Intent::MenuUpsert { draft }) => {
                assert_eq!(draft.name, "mango pudding");
                assert_eq!(draft.price, "3.20");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn garbage_is_none() {
        assert!(intent_from_model_json("sorry I cannot").is_none());
        assert!(intent_from_model_json("{\"intent\":\"nope\"}").is_none());
        assert!(intent_from_model_json("").is_none());
    }
}
