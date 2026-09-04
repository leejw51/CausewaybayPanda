//! Proof that a wallet payment happened. A browser reports a hash; the chain's
//! own RPC says what that hash did. Nothing is booked until the receipt shows
//! a successful ERC-20 `Transfer` of at least the bill, from the USDC contract,
//! to the shop's treasury.

use std::time::Duration;

use serde_json::{json, Value};

/// `keccak256("Transfer(address,address,uint256)")`, the topic every ERC-20
/// emits on a transfer.
pub const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// What a good receipt proved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub from: String,
    pub amount: u128,
    pub block: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The chain has not seen the hash yet, or has not mined it.
    Pending,
    /// Mined and it paid the bill.
    Paid(Transfer),
    /// Mined and it did not: reverted, wrong token, wrong payee.
    Rejected(String),
    /// Mined, paid this shop, but not enough. Phrased by the caller, which
    /// knows what the board reads in.
    Underpaid { paid: u128, needed: u128 },
}

/// Judge a receipt against what the shop expected. Pure, so a test can hand
/// in any JSON the chain might answer with.
pub fn judge(receipt: &Value, token: &str, treasury: &str, min_amount: u128) -> Verdict {
    if receipt.is_null() {
        return Verdict::Pending;
    }
    let status = receipt.get("status").and_then(Value::as_str).unwrap_or("");
    if hex_to_u128(status) != Some(1) {
        return Verdict::Rejected("the transaction reverted".into());
    }
    let Some(logs) = receipt.get("logs").and_then(Value::as_array) else {
        return Verdict::Rejected("the transaction emitted nothing".into());
    };
    let token = token.to_lowercase();
    let want_to = topic_address(treasury);
    let block = receipt
        .get("blockNumber")
        .and_then(Value::as_str)
        .and_then(hex_to_u128)
        .unwrap_or(0) as u64;

    // Several transfers can share one transaction; the one that matters is
    // from the USDC contract, to the treasury. Sum them in case of a split.
    let mut paid: u128 = 0;
    let mut from = String::new();
    let mut saw_token = false;
    for log in logs {
        let address = log
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        if address != token {
            continue;
        }
        saw_token = true;
        let topics: Vec<&str> = log
            .get("topics")
            .and_then(Value::as_array)
            .map(|t| t.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if topics.len() != 3 || !topics[0].eq_ignore_ascii_case(TRANSFER_TOPIC) {
            continue;
        }
        if !topics[2].eq_ignore_ascii_case(&want_to) {
            continue;
        }
        let Some(amount) = log
            .get("data")
            .and_then(Value::as_str)
            .and_then(hex_to_u128)
        else {
            continue;
        };
        paid = paid.saturating_add(amount);
        if from.is_empty() {
            from = format!("0x{}", &topics[1][topics[1].len() - 40..]);
        }
    }
    if !saw_token {
        return Verdict::Rejected("that transaction did not move USDC".into());
    }
    if paid == 0 {
        return Verdict::Rejected("that transaction did not pay this shop".into());
    }
    if paid < min_amount {
        return Verdict::Underpaid {
            paid,
            needed: min_amount,
        };
    }
    Verdict::Paid(Transfer {
        from,
        amount: paid,
        block,
    })
}

/// Ask the chain, and keep asking until the block lands or patience runs out.
/// Cronos mines about every six seconds; this waits up to `wait` in total.
pub async fn confirm(
    rpc_url: &str,
    tx_hash: &str,
    token: &str,
    treasury: &str,
    min_amount: u128,
    wait: Duration,
) -> Verdict {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Verdict::Rejected(format!("no http client: {e}")),
    };
    let step = Duration::from_secs(2);
    let started = std::time::Instant::now();
    loop {
        match receipt(&client, rpc_url, tx_hash).await {
            Ok(r) => match judge(&r, token, treasury, min_amount) {
                Verdict::Pending => {}
                done => return done,
            },
            Err(e) => {
                if started.elapsed() >= wait {
                    return Verdict::Rejected(format!("could not reach {rpc_url}: {e}"));
                }
            }
        }
        if started.elapsed() >= wait {
            return Verdict::Pending;
        }
        tokio::time::sleep(step).await;
    }
}

async fn receipt(client: &reqwest::Client, rpc_url: &str, tx_hash: &str) -> Result<Value, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error") {
        return Err(err.to_string());
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// A 20-byte address as it appears in an indexed log topic: 12 zero bytes,
/// then the address, lowercase.
fn topic_address(addr: &str) -> String {
    let bare = addr
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    format!("0x{}{}", "0".repeat(24), bare.to_lowercase())
}

fn hex_to_u128(s: &str) -> Option<u128> {
    let bare = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if bare.is_empty() {
        return Some(0);
    }
    // A 32-byte word is 64 hex digits; u128 holds 32. Any value that large
    // is not a USDC amount this shop will ever see.
    let bare = bare.trim_start_matches('0');
    if bare.len() > 32 {
        return None;
    }
    if bare.is_empty() {
        return Some(0);
    }
    u128::from_str_radix(bare, 16).ok()
}

/// Clamp an on-chain amount into the shop's own integer range.
pub fn as_micro(n: u128) -> i64 {
    n.min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    const USDC: &str = "0xc21223249CA28397B4B6541dfFaEcC539BfF0c59";
    const SHOP: &str = "0x2222222222222222222222222222222222222222";
    const GUEST: &str = "0x3333333333333333333333333333333333333333";

    /// A receipt the way Cronos returns one for a plain USDC transfer.
    fn receipt(status: &str, token: &str, to: &str, amount: u128) -> Value {
        json!({
            "status": status,
            "blockNumber": "0x1a2b3c",
            "to": token,
            "logs": [{
                "address": token,
                "topics": [TRANSFER_TOPIC, topic_address(GUEST), topic_address(to)],
                "data": format!("0x{amount:064x}"),
            }],
        })
    }

    #[test]
    fn a_successful_transfer_of_the_bill_is_paid() {
        let v = judge(
            &receipt("0x1", USDC, SHOP, 4_800_000),
            USDC,
            SHOP,
            4_800_000,
        );
        match v {
            Verdict::Paid(t) => {
                assert_eq!(t.amount, 4_800_000);
                assert_eq!(t.from, GUEST.to_lowercase());
                assert_eq!(t.block, 0x1a2b3c);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn overpaying_is_the_guests_choice_and_still_pays() {
        assert!(matches!(
            judge(
                &receipt("0x1", USDC, SHOP, 5_000_000),
                USDC,
                SHOP,
                4_800_000
            ),
            Verdict::Paid(_)
        ));
    }

    #[test]
    fn a_hash_the_chain_has_not_seen_is_pending_not_paid() {
        assert_eq!(judge(&Value::Null, USDC, SHOP, 1), Verdict::Pending);
    }

    #[test]
    fn a_reverted_transaction_pays_nothing() {
        assert!(matches!(
            judge(&receipt("0x0", USDC, SHOP, 4_800_000), USDC, SHOP, 4_800_000),
            Verdict::Rejected(m) if m.contains("reverted")
        ));
    }

    #[test]
    fn underpaying_is_refused_with_both_numbers() {
        match judge(
            &receipt("0x1", USDC, SHOP, 1_000_000),
            USDC,
            SHOP,
            4_800_000,
        ) {
            Verdict::Underpaid { paid, needed } => {
                assert_eq!(paid, 1_000_000);
                assert_eq!(needed, 4_800_000);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_transfer_to_someone_else_is_not_a_payment() {
        let elsewhere = "0x4444444444444444444444444444444444444444";
        assert!(matches!(
            judge(&receipt("0x1", USDC, elsewhere, 4_800_000), USDC, SHOP, 4_800_000),
            Verdict::Rejected(m) if m.contains("did not pay this shop")
        ));
    }

    #[test]
    fn a_transfer_of_some_other_token_is_not_a_payment() {
        let other_token = "0x66e428c3f67a68878562e79A0234c1F83c208770"; // USDT
        assert!(matches!(
            judge(&receipt("0x1", other_token, SHOP, 4_800_000), USDC, SHOP, 4_800_000),
            Verdict::Rejected(m) if m.contains("did not move USDC")
        ));
    }

    #[test]
    fn addresses_compare_without_regard_to_case() {
        let v = judge(
            &receipt(
                "0x1",
                &USDC.to_uppercase().replace("0X", "0x"),
                &SHOP.to_uppercase().replace("0X", "0x"),
                4_800_000,
            ),
            &USDC.to_lowercase(),
            SHOP,
            4_800_000,
        );
        assert!(matches!(v, Verdict::Paid(_)), "{v:?}");
    }

    #[test]
    fn two_transfers_in_one_transaction_add_up() {
        let mut r = receipt("0x1", USDC, SHOP, 3_000_000);
        r["logs"]
            .as_array_mut()
            .unwrap()
            .push(receipt("0x1", USDC, SHOP, 1_800_000)["logs"][0].clone());
        match judge(&r, USDC, SHOP, 4_800_000) {
            Verdict::Paid(t) => assert_eq!(t.amount, 4_800_000),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hex_words_parse_and_absurd_ones_do_not() {
        assert_eq!(hex_to_u128("0x1"), Some(1));
        assert_eq!(hex_to_u128("0x0"), Some(0));
        assert_eq!(hex_to_u128("0x"), Some(0));
        assert_eq!(
            hex_to_u128(&format!("0x{:064x}", 4_800_000u128)),
            Some(4_800_000)
        );
        assert_eq!(hex_to_u128(&format!("0x{}", "f".repeat(64))), None);
    }
}
