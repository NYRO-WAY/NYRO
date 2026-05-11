use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub proxy_bind_addr: String,
    pub admin_bind_addr: Option<String>,
    pub request_timeout_secs: u64,
    pub database_url: String,
    pub bootstrap_json_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_bind_addr: "127.0.0.1:2080".to_string(),
            admin_bind_addr: None,
            request_timeout_secs: 300,
            database_url: "sqlite://upstream-gateway.db".to_string(),
            bootstrap_json_path: None,
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(value) = env::var("UPSTREAM_GATEWAY_PROXY_BIND_ADDR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.proxy_bind_addr = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("UPSTREAM_GATEWAY_ADMIN_BIND_ADDR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.admin_bind_addr = Some(trimmed.to_string());
            }
        }

        if let Ok(value) = env::var("UPSTREAM_GATEWAY_BIND_ADDR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                if env::var("UPSTREAM_GATEWAY_PROXY_BIND_ADDR").is_err() {
                    config.proxy_bind_addr = trimmed.to_string();
                }
            }
        }

        if let Ok(value) = env::var("UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS")
            && let Ok(parsed) = value.trim().parse::<u64>()
            && parsed > 0
        {
            config.request_timeout_secs = parsed;
        }

        if let Ok(value) = env::var("UPSTREAM_GATEWAY_DATABASE_URL") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.database_url = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.bootstrap_json_path = Some(trimmed.to_string());
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::AppConfig;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_gateway_env() {
        for key in [
            "UPSTREAM_GATEWAY_PROXY_BIND_ADDR",
            "UPSTREAM_GATEWAY_ADMIN_BIND_ADDR",
            "UPSTREAM_GATEWAY_BIND_ADDR",
            "UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS",
            "UPSTREAM_GATEWAY_DATABASE_URL",
            "UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH",
        ] {
            unsafe {
                std::env::remove_var(key);
            }
        }
    }

    #[test]
    fn defaults_to_proxy_only_when_admin_is_unset() {
        let _guard = env_lock().lock().unwrap();
        clear_gateway_env();
        let config = AppConfig::from_env();
        assert_eq!(config.proxy_bind_addr, "127.0.0.1:2080");
        assert_eq!(config.admin_bind_addr, None);
    }

    #[test]
    fn admin_listener_is_enabled_only_when_explicitly_configured() {
        let _guard = env_lock().lock().unwrap();
        clear_gateway_env();
        unsafe {
            std::env::set_var("UPSTREAM_GATEWAY_ADMIN_BIND_ADDR", "127.0.0.1:2081");
        }
        let config = AppConfig::from_env();
        assert_eq!(config.admin_bind_addr.as_deref(), Some("127.0.0.1:2081"));
        clear_gateway_env();
    }

    #[test]
    fn legacy_bind_addr_does_not_enable_admin_by_itself() {
        let _guard = env_lock().lock().unwrap();
        clear_gateway_env();
        unsafe {
            std::env::set_var("UPSTREAM_GATEWAY_BIND_ADDR", "0.0.0.0:2080");
        }
        let config = AppConfig::from_env();
        assert_eq!(config.proxy_bind_addr, "0.0.0.0:2080");
        assert_eq!(config.admin_bind_addr, None);
        clear_gateway_env();
    }
}
