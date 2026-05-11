use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub proxy_bind_addr: String,
    pub admin_bind_addr: String,
    pub database_url: String,
    pub bootstrap_json_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_bind_addr: "127.0.0.1:2090".to_string(),
            admin_bind_addr: "127.0.0.1:2091".to_string(),
            database_url: "sqlite://race-gateway.db".to_string(),
            bootstrap_json_path: None,
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(value) = env::var("RACE_GATEWAY_PROXY_BIND_ADDR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.proxy_bind_addr = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("RACE_GATEWAY_ADMIN_BIND_ADDR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.admin_bind_addr = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("RACE_GATEWAY_DATABASE_URL") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.database_url = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("RACE_GATEWAY_BOOTSTRAP_JSON_PATH") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                config.bootstrap_json_path = Some(trimmed.to_string());
            }
        }

        config
    }
}
