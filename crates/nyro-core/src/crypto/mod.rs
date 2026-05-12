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

    #[cfg(not(target_env = "musl"))]
    let key = get_key_from_keyring()?;

    #[cfg(target_env = "musl")]
    let key = get_key_from_env_or_file()?;

    let _ = MASTER_KEY_CACHE.set(key);
    Ok(key)
}

/// 通过系统 keyring（Secret Service / Keychain / Credential Manager）存取主密钥。
/// 仅在非 musl 构建下编译，因为 Linux Secret Service 依赖 dbus 动态库，无法静态链接。
#[cfg(not(target_env = "musl"))]
fn get_key_from_keyring() -> anyhow::Result<[u8; 32]> {
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

    Ok(key)
}

/// musl 静态构建下的主密钥获取策略（不依赖任何系统动态库）：
///
/// 1. 环境变量 `NYRO_MASTER_KEY`（base64, >= 32 bytes）—— 推荐用于容器/K8s
/// 2. 文件 `~/.local/share/nyro/master.key`（首次运行自动生成）—— 适合裸机部署
/// 3. 进程内随机 key（降级兜底，重启后密文不可用，启动时打印警告）
#[cfg(target_env = "musl")]
fn get_key_from_env_or_file() -> anyhow::Result<[u8; 32]> {
    if let Ok(val) = std::env::var("NYRO_MASTER_KEY") {
        let bytes = base64::engine::general_purpose::STANDARD.decode(val.trim())?;
        if bytes.len() < 32 {
            anyhow::bail!("NYRO_MASTER_KEY must decode to at least 32 bytes");
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes[..32]);
        return Ok(key);
    }

    if let Some(key) = try_load_or_create_key_file() {
        return Ok(key);
    }

    tracing::warn!(
        "No persistent master key found (set NYRO_MASTER_KEY or ensure ~/.local/share/nyro is writable). \
         Using a per-process random key — encrypted values will not survive restarts."
    );
    let raw = Aes256Gcm::generate_key(OsRng);
    let mut key = [0u8; 32];
    key.copy_from_slice(raw.as_slice());
    Ok(key)
}

#[cfg(target_env = "musl")]
fn try_load_or_create_key_file() -> Option<[u8; 32]> {
    use std::io::{Read, Write};

    let dir = dirs::data_local_dir()?.join("nyro");
    let path = dir.join("master.key");

    if path.exists() {
        let mut f = std::fs::File::open(&path).ok()?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).ok()?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(buf.trim())
            .ok()?;
        if bytes.len() < 32 {
            return None;
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes[..32]);
        return Some(key);
    }

    std::fs::create_dir_all(&dir).ok()?;
    let raw = Aes256Gcm::generate_key(OsRng);
    let b64 = base64::engine::general_purpose::STANDARD.encode(raw.as_slice());
    let mut f = std::fs::File::create(&path).ok()?;
    f.write_all(b64.as_bytes()).ok()?;

    // 权限设为 0600，仅所有者可读
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(raw.as_slice());
    Some(key)
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
