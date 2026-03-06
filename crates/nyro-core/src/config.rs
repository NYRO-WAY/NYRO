use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub proxy_host: String,
    pub proxy_port: u16,
    pub data_dir: PathBuf,
    pub auth_key: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 19530,
            data_dir: default_data_dir(),
            auth_key: None,
        }
    }
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nyro")
}
