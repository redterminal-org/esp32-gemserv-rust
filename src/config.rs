use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub wifi: WifiConfig,
    pub server: ServerConfig,
    pub tls: TlsConfig,
    pub gemini: GeminiConfig,
}

#[derive(Debug, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct TlsConfig {
    pub certificate_path: String,
    pub private_key_path: String,
    pub handshake_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeminiConfig {
    pub request_line_max_bytes: usize,
    pub response_chunk_size: usize,
    pub request_timeout_seconds: u64,
    pub lang: Option<String>,
    pub timezone: Option<String>,
    pub boot_log_path: Option<String>,
}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&contents)?;
    Ok(config)
}
