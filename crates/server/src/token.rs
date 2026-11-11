//! 席トークン。ログインの代わりに、対局作成時に人間の各席へ発行する。
//! DB にはハッシュだけを保存する。

use sha2::{Digest, Sha256};

/// ランダムな席トークン(256bit、16 進 64 文字)を作る。
pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS の乱数源が使えません");
    hex::encode(bytes)
}

/// トークンの SHA-256(16 進)。
pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_well_formed() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_is_sha256_hex() {
        // SHA-256("abc") の既知の値。
        assert_eq!(hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_ne!(hash("abc"), hash("abd"));
        // トークンそのものは含まない。
        let token = generate();
        assert_ne!(hash(&token), token);
        assert_eq!(hash(&token).len(), 64);
    }
}
