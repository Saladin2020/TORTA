//! At-rest encryption for the shared org chunk store.
//!
//! Content hashing stays on plaintext so **org-wide dedup is preserved**.
//! Ciphertext is deterministic per (org_key, chunk_hash) via a hash-derived nonce.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};

use crate::error::{DiceError, Result};

pub const MAGIC: &[u8; 4] = b"DCE1";

#[derive(Clone)]
pub struct StoreCipher {
    key: [u8; 32],
}

impl StoreCipher {
    /// 32-byte key as hex (64 chars) or raw 32 bytes accepted via env decode elsewhere.
    pub fn from_hex(hex_key: &str) -> Result<Self> {
        let hex_key = hex_key.trim();
        if hex_key.len() != 64 {
            return Err(DiceError::msg(
                "store encryption key must be 64 hex chars (32 bytes)",
            ));
        }
        let mut key = [0u8; 32];
        for i in 0..32 {
            let byte = u8::from_str_radix(&hex_key[i * 2..i * 2 + 2], 16)
                .map_err(|e| DiceError::msg(format!("invalid hex key: {e}")))?;
            key[i] = byte;
        }
        Ok(Self { key })
    }

    pub fn from_bytes(key: [u8; 32]) -> Self {
        Self { key }
    }

    fn nonce_for(hash: &str) -> Nonce {
        let digest = blake3::hash(hash.as_bytes());
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&digest.as_bytes()[..12]);
        Nonce::from(nonce)
    }

    /// Encrypt compressed chunk bytes for disk. Output = MAGIC || ciphertext+tag.
    pub fn seal(&self, hash: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key)
            .map_err(|e| DiceError::msg(format!("cipher init: {e}")))?;
        let nonce = Self::nonce_for(hash);
        let ct = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| DiceError::msg(format!("encrypt: {e}")))?;
        let mut out = Vec::with_capacity(MAGIC.len() + ct.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Decrypt if MAGIC present; otherwise return bytes unchanged (legacy plaintext chunks).
    pub fn open(&self, hash: &str, blob: &[u8]) -> Result<Vec<u8>> {
        if blob.len() >= 4 && &blob[..4] == MAGIC {
            let cipher = ChaCha20Poly1305::new_from_slice(&self.key)
                .map_err(|e| DiceError::msg(format!("cipher init: {e}")))?;
            let nonce = Self::nonce_for(hash);
            return cipher
                .decrypt(&nonce, &blob[4..])
                .map_err(|e| DiceError::msg(format!("decrypt chunk {hash}: {e}")));
        }
        Ok(blob.to_vec())
    }

    pub fn is_encrypted_blob(blob: &[u8]) -> bool {
        blob.len() >= 4 && &blob[..4] == MAGIC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_and_deterministic() {
        let c = StoreCipher::from_hex(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        let hash = "abc123";
        let pt = b"compressed-chunk-bytes";
        let a = c.seal(hash, pt).unwrap();
        let b = c.seal(hash, pt).unwrap();
        assert_eq!(a, b, "deterministic seal preserves dedup");
        assert!(StoreCipher::is_encrypted_blob(&a));
        assert_eq!(c.open(hash, &a).unwrap(), pt);
    }
}
