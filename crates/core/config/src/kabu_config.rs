use crate::config_loader::{ConfigLoader, ConfigLoaderSync, LoadConfigError, load_from_file, load_from_file_sync};
use alloy_primitives::Address;
use async_trait::async_trait;
use eyre::Result;
use serde::Deserialize;
use std::collections::HashMap;
use strum_macros::Display;

/// Node type (Geth or Reth)
#[derive(Clone, Debug, Default, Deserialize, Display)]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    #[default]
    Geth,
    Reth,
}

/// Remote node configuration
#[derive(Clone, Debug, Deserialize)]
pub struct RemoteNodeConfig {
    /// WebSocket URL for the node (e.g., ws://localhost:8545)
    pub ws_url: String,
    /// HTTP URL for the node (e.g., http://localhost:8545)
    pub http_url: String,
    /// Node type (Geth or Reth)
    pub node: NodeType,
}

impl RemoteNodeConfig {
    /// Get the WebSocket URL
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Get the HTTP URL
    pub fn http_url(&self) -> &str {
        &self.http_url
    }
}

/// Signer configuration
#[derive(Clone, Debug, Deserialize)]
pub struct SignerConfig {
    pub private_key: String,
}

/// Builder (relay) configuration for MEV submission
#[derive(Clone, Debug, Deserialize)]
pub struct BuilderConfig {
    pub id: u8,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub no_sign: bool,
}

/// InfluxDB configuration for metrics
#[derive(Clone, Debug, Default, Deserialize)]
pub struct InfluxDbConfig {
    pub url: String,
    pub database: String,
    pub tags: HashMap<String, String>,
}

/// Web server configuration
#[derive(Clone, Debug, Deserialize)]
pub struct WebserverConfig {
    pub host: String,
}

impl Default for WebserverConfig {
    fn default() -> Self {
        WebserverConfig { host: "127.0.0.1:3333".to_string() }
    }
}

/// Database configuration
#[derive(Clone, Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

/// Main Kabu configuration
#[derive(Clone, Debug, Deserialize)]
pub struct KabuConfig {
    /// Optional remote node configuration (for remote mode)
    pub remote_node: Option<RemoteNodeConfig>,

    /// Multiple signers for redundancy and balance management
    pub signers: Vec<SignerConfig>,

    /// MEV builders/relays for transaction submission
    #[serde(default)]
    pub builders: Vec<BuilderConfig>,

    /// Multicaller contract address
    pub multicaller_address: Address,

    /// Optional InfluxDB configuration
    pub influxdb: Option<InfluxDbConfig>,

    /// Optional web server configuration
    pub webserver: Option<WebserverConfig>,

    /// Optional database configuration
    pub database: Option<DatabaseConfig>,
}

impl KabuConfig {
    /// Load configuration from file (sync version)
    pub fn load_from_file(file_name: String) -> Result<KabuConfig> {
        let config = load_from_file_sync(file_name)?;
        Ok(config)
    }

    /// Load configuration from file (async version)
    pub async fn load_from_file_async(file_name: String) -> Result<KabuConfig> {
        let config = load_from_file(file_name).await?;
        Ok(config)
    }
}

#[async_trait]
impl ConfigLoader for KabuConfig {
    type SectionType = KabuConfig;

    async fn load_from_file(file_name: String) -> Result<Self::SectionType, LoadConfigError> {
        load_from_file(file_name).await
    }
}

impl ConfigLoaderSync for KabuConfig {
    type SectionType = KabuConfig;

    fn load_from_file_sync(file_name: String) -> Result<Self::SectionType, LoadConfigError> {
        load_from_file_sync(file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_display() {
        assert_eq!(NodeType::Geth.to_string(), "geth");
        assert_eq!(NodeType::Reth.to_string(), "reth");
    }

    #[test]
    fn test_remote_node_config() {
        let config = RemoteNodeConfig {
            ws_url: "ws://localhost:8545".to_string(),
            http_url: "http://localhost:8545".to_string(),
            node: NodeType::Reth,
        };

        assert_eq!(config.ws_url(), "ws://localhost:8545");
        assert_eq!(config.http_url(), "http://localhost:8545");
    }

    #[test]
    fn test_config_parsing_with_remote_node() {
        let toml_str = r#"
            signers = []
            builders = []
            multicaller_address = "0x0000000000000000000000000000000000000000"

            [remote_node]
            ws_url = "ws://localhost:8545"
            http_url = "http://localhost:8545"
            node = "reth"
        "#;

        let config: KabuConfig = toml::from_str(toml_str).unwrap();
        assert!(config.remote_node.is_some());
        let remote = config.remote_node.unwrap();
        assert_eq!(remote.ws_url, "ws://localhost:8545");
        assert_eq!(remote.http_url, "http://localhost:8545");
    }

    #[test]
    fn test_config_parsing_without_remote_node() {
        let toml_str = r#"
            signers = []
            builders = []
            multicaller_address = "0x0000000000000000000000000000000000000000"
        "#;

        let config: KabuConfig = toml::from_str(toml_str).unwrap();
        assert!(config.remote_node.is_none());
    }
}
