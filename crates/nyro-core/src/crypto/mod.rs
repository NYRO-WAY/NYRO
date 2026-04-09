use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use std::sync::OnceLock;

const KEYRING_SERVICE: &str = "nyro-gateway";
const KEYRING_USER: &str = "master-key";
const NONCE_LEN: usize = 12;
static MASTER_KEY_CACHE: OnceLock<[u8; 32]> = OnceLock::new();

fn get_or_create_master_key() -> anyhow::Result<[u8; 32]> {
    if let Some(key) = MASTER_KEY_CACHE.get() {
        return Ok(*key);
    }

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;

    let key = match entry.get_password() {
        Ok(b64) => {
            let bytes = base64::engine::general_purpose::STANDARD.decode(&b64)?;
            let mut key = [0u8; 32];
            if bytes.len() >= 32 {
                key.copy_from_slice(&bytes[..32]);
            }
            key
        }
        Err(_) => {
            let key = Aes256Gcm::generate_key(OsRng);
            let b64 = base64::engine::general_purpose::STANDARD.encode(key.as_slice());
            entry
                .set_password(&b64)
                .map_err(|e| anyhow::anyhow!("failed to persist master key to keyring: {e}"))?;
            let mut arr = [0u8; 32];
            arr.copy_from_slice(key.as_slice());
            arr
        }
    };

    let _ = MASTER_KEY_CACHE.set(key);
    Ok(key)
}

pub fn encrypt(plaintext: &str) -> String {
    let Ok(key_bytes) = get_or_create_master_key() else {
        return plaintext.to_string();
    };

    let cipher = Aes256Gcm::new_from_slice(&key_bytes).unwrap();
    let nonce_bytes = aes_gcm::aead::generic_array::GenericArray::from(rand_nonce());
    let nonce = Nonce::from(nonce_bytes);

    match cipher.encrypt(&nonce, plaintext.as_bytes()) {
        Ok(ciphertext) => {
            let mut out = nonce_bytes.to_vec();
            out.extend_from_slice(&ciphertext);
            format!(
                "enc:{}",
                base64::engine::general_purpose::STANDARD.encode(&out)
            )
        }
        Err(_) => plaintext.to_string(),
    }
}

pub fn decrypt(ciphertext: &str) -> String {
    let Some(b64) = ciphertext.strip_prefix("enc:") else {
        return ciphertext.to_string();
    };

    let Ok(key_bytes) = get_or_create_master_key() else {
        return ciphertext.to_string();
    };

    let Ok(data) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        return ciphertext.to_string();
    };

    if data.len() < NONCE_LEN + 1 {
        return ciphertext.to_string();
    }

    let (nonce_bytes, ct) = data.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).unwrap();

    match cipher.decrypt(nonce, ct) {
        Ok(plaintext) => String::from_utf8(plaintext).unwrap_or_else(|_| ciphertext.to_string()),
        Err(_) => ciphertext.to_string(),
    }
}

pub fn decrypt_nested(ciphertext: &str) -> String {
    let mut current = ciphertext.to_string();
    for _ in 0..3 {
        let next = decrypt(&current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn rand_nonce() -> [u8; NONCE_LEN] {
    use aes_gcm::aead::rand_core::RngCore;
    let mut buf = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let original = "sk-test-key-12345";
        let encrypted = encrypt(original);
        assert!(encrypted.starts_with("enc:"));
        let decrypted = decrypt(&encrypted);
        assert_eq!(decrypted, original);
    }

    #[test]
    fn plaintext_passthrough() {
        assert_eq!(decrypt("sk-plain"), "sk-plain");
    }
}
