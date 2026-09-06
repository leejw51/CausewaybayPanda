//! `transfer(address,uint256)` calldata, byte-for-byte the encoding in
//! Causewaybay Wallet `rustcli/core/src/erc20.rs`.

/// `transfer(address,uint256)` — `keccak256("transfer(address,uint256)")[..4]`
pub const SELECTOR_TRANSFER: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];

/// `balanceOf(address)` — `keccak256("balanceOf(address)")[..4]`
pub const SELECTOR_BALANCE_OF: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

/// `balanceOf(holder)` calldata, for an `eth_call` against the token.
pub fn encode_balance_of_hex(holder: &[u8; 20]) -> String {
    let mut s = String::from("0x");
    for b in SELECTOR_BALANCE_OF.iter().chain(pad_address(holder).iter()) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Encode an ERC-20 transfer to `to` of `amount` atomic units (USDC: 6 decimals).
pub fn encode_transfer(to: &[u8; 20], amount: u128) -> Vec<u8> {
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&SELECTOR_TRANSFER);
    data.extend_from_slice(&pad_address(to));
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&amount.to_be_bytes());
    data.extend_from_slice(&word);
    data
}

fn pad_address(address: &[u8; 20]) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(address);
    word
}

pub fn encode_transfer_hex(to: &[u8; 20], amount: u128) -> String {
    let bytes = encode_transfer(to, amount);
    let mut s = String::from("0x");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Parse `0x` + 40 hex chars into a 20-byte address. Case-insensitive.
pub fn parse_address(s: &str) -> Option<[u8; 20]> {
    let t = s
        .trim()
        .strip_prefix("0x")
        .or_else(|| s.trim().strip_prefix("0X"))?;
    if t.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for i in 0..20 {
        out[i] = u8::from_str_radix(&t[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_and_padding() {
        let to = parse_address("0x1111111111111111111111111111111111111111").unwrap();
        let data = encode_transfer(&to, 4_800_000);
        assert_eq!(&data[..4], &SELECTOR_TRANSFER);
        assert_eq!(data.len(), 68);
        assert_eq!(&data[16..36], &to);
        let mut amount_word = [0u8; 32];
        amount_word[16..].copy_from_slice(&4_800_000u128.to_be_bytes());
        assert_eq!(&data[36..68], &amount_word);
        assert!(encode_transfer_hex(&to, 1).starts_with("0xa9059cbb"));
        let bal = encode_balance_of_hex(&to);
        assert!(bal.starts_with("0x70a08231"));
        assert_eq!(bal.len(), 2 + 8 + 64);
        assert!(bal.ends_with(&"11".repeat(20)));
        assert_eq!(parse_address("not-an-address"), None);
        assert_eq!(parse_address("0x11"), None);
    }
}
