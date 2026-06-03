//! Configuration loading for Strait.
//!
//! Loads configuration from environment variables with `.env` file fallback.
//! Uses the `config` crate for layered configuration with sensible defaults.

use crate::error::{Result, StraitError};
use serde::Deserialize;

/// Top-level application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub bitcoin: BitcoinConfig,
    pub hemi: EvmChainConfig,
    pub ethereum: EvmChainConfig,
    pub database: DatabaseConfig,
    pub api: ApiConfig,
}

/// Bitcoin chain configuration
#[derive(Debug, Clone, Deserialize)]
pub struct BitcoinConfig {
    /// Bitcoin RPC URL (e.g., http://localhost:8332)
    pub rpc_url: String,
    /// Bitcoin RPC username
    pub rpc_user: String,
    /// Bitcoin RPC password
    pub rpc_password: String,
    /// Comma-separated list of tunnel custody addresses to watch
    pub tunnel_addresses: Vec<String>,
    /// Number of confirmations before considering a block final
    #[serde(default = "default_bitcoin_confirmation_depth")]
    pub confirmation_depth: u32,
    /// Poll interval in seconds for checking new blocks
    #[serde(default = "default_bitcoin_poll_interval")]
    pub poll_interval_secs: u64,
}

/// EVM chain configuration (used for both Hemi and Ethereum)
#[derive(Debug, Clone, Deserialize)]
pub struct EvmChainConfig {
    /// RPC URL for the chain
    pub rpc_url: String,
    /// Chain ID (43111 for Hemi mainnet, 743111 for Hemi testnet, 1 for Ethereum mainnet, 11155111 for Sepolia)
    pub chain_id: u64,
    /// Tunnel contract address (checksummed).
    /// For ETH/ERC-20 routes: L2StandardBridge (0x4200000000000000000000000000000000000010 on Hemi).
    /// For BTC routes: the Hemi Bitcoin tunnel contract (address TBC — FIXME: confirm with Hemi docs).
    pub tunnel_contract: String,
    /// BitcoinKit precompile address on Hemi.
    /// Mainnet: 0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12 (v1)
    /// Testnet: 0xeC9fa5daC1118963933e1A675a4EEA0009b7f215 (v0)
    /// Only relevant for the Hemi chain config — ignored for Ethereum.
    #[serde(default)]
    pub bitcoin_kit_contract: Option<String>,
    /// Block number to start indexing from
    #[serde(default = "default_start_block")]
    pub start_block: u64,
    /// Number of confirmations before considering a block final
    #[serde(default = "default_evm_confirmation_depth")]
    pub confirmation_depth: u32,
    /// Poll interval in milliseconds for checking new blocks
    #[serde(default = "default_evm_poll_interval")]
    pub poll_interval_ms: u64,
}

/// Database configuration
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// PostgreSQL connection string
    pub url: String,
    /// Maximum number of database connections
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

/// API server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    /// Host to bind the API server to
    #[serde(default = "default_api_host")]
    pub host: String,
    /// Port to bind the API server to
    #[serde(default = "default_api_port")]
    pub port: u16,
}

// ============================================================================
// Confirmed timing constants (from Hemi documentation)
// ============================================================================

/// Hemi keystone frequency — every 25 Hemi blocks a keystone is produced.
/// Must match PoPPayoutsV2.KEYSTONE_FREQUENCY = 25.
pub const KEYSTONE_FREQUENCY: u32 = 25;

/// Bitcoin tunnel: 6 confirmations required before minting on Hemi (~1 hour).
pub const BTC_CONFIRMATION_DEPTH: u32 = 6;

/// Hemi: faster finality, 3 confirmations sufficient.
pub const HEMI_CONFIRMATION_DEPTH: u32 = 3;

/// Ethereum: 12 confirmations for standard finality.
pub const ETH_CONFIRMATION_DEPTH: u32 = 12;

/// ETH→Hemi deposit: representative token mints within ~2 minutes of Ethereum lock.
/// Used as the match window for pairing TunnelLock with TunnelMint events.
pub const ETH_DEPOSIT_MATCH_WINDOW_SECS: i64 = 300; // 5 min — 2.5x the observed ~2 min

/// Hemi→ETH withdrawal: up to ~40 minutes to achieve Bitcoin finality,
/// plus up to 24 hours for proof submission. The join engine keeps a
/// withdrawal in ANCHORED state for this long before marking it FAILED.
pub const ETH_WITHDRAWAL_PROOF_WINDOW_SECS: i64 = 90_000; // 25 hours

/// BTC→Hemi deposit: 6 Bitcoin confirmations at ~10 min/block = ~1 hour.
/// Used as the match window for pairing TunnelDeposit with TunnelMint events.
pub const BTC_DEPOSIT_MATCH_WINDOW_SECS: i64 = 7_200; // 2 hours — 2x the observed ~1 hour

/// Hemi→BTC withdrawal: up to ~12 hours for custodian verification.
pub const BTC_WITHDRAWAL_WINDOW_SECS: i64 = 50_400; // 14 hours — safety margin

// ============================================================================
// Default values
// ============================================================================

fn default_bitcoin_confirmation_depth() -> u32 {
    BTC_CONFIRMATION_DEPTH
}

fn default_bitcoin_poll_interval() -> u64 {
    10
}

fn default_start_block() -> u64 {
    0
}

fn default_evm_confirmation_depth() -> u32 {
    ETH_CONFIRMATION_DEPTH
}

fn default_evm_poll_interval() -> u64 {
    1000
}

fn default_max_connections() -> u32 {
    10
}

fn default_api_host() -> String {
    "0.0.0.0".to_string()
}

fn default_api_port() -> u16 {
    8080
}

// ============================================================================
// Configuration loading
// ============================================================================

impl AppConfig {
    /// Load configuration from environment variables and .env file
    pub fn load() -> Result<Self> {
        // Load .env file if it exists (ignore if not found)
        let _ = dotenvy::dotenv();

        let config = config::Config::builder()
            // Bitcoin
            .set_default(
                "bitcoin.confirmation_depth",
                default_bitcoin_confirmation_depth(),
            )?
            .set_default(
                "bitcoin.poll_interval_secs",
                default_bitcoin_poll_interval(),
            )?
            // Hemi
            .set_default("hemi.start_block", default_start_block())?
            .set_default("hemi.confirmation_depth", 3u32)? // Hemi has faster finality
            .set_default("hemi.poll_interval_ms", default_evm_poll_interval())?
            // Ethereum
            .set_default("ethereum.start_block", default_start_block())?
            .set_default(
                "ethereum.confirmation_depth",
                default_evm_confirmation_depth(),
            )?
            .set_default("ethereum.poll_interval_ms", default_evm_poll_interval())?
            // Database
            .set_default("database.max_connections", default_max_connections())?
            // API
            .set_default("api.host", default_api_host())?
            .set_default("api.port", default_api_port())?
            // Load from environment
            .add_source(config::Environment::with_prefix("STRAIT").separator("__"))
            .build()?;

        let app_config: AppConfig = config.try_deserialize()?;

        // Validate required fields
        app_config.validate()?;

        Ok(app_config)
    }

    /// Validate that all required configuration is present
    fn validate(&self) -> Result<()> {
        if self.bitcoin.rpc_url.is_empty() {
            return Err(StraitError::MissingConfig(
                "BITCOIN_RPC_URL is required".to_string(),
            ));
        }
        if self.bitcoin.rpc_user.is_empty() {
            return Err(StraitError::MissingConfig(
                "BITCOIN_RPC_USER is required".to_string(),
            ));
        }
        if self.bitcoin.rpc_password.is_empty() {
            return Err(StraitError::MissingConfig(
                "BITCOIN_RPC_PASSWORD is required".to_string(),
            ));
        }
        if self.hemi.rpc_url.is_empty() {
            return Err(StraitError::MissingConfig(
                "HEMI_RPC_URL is required".to_string(),
            ));
        }
        if self.hemi.tunnel_contract.is_empty() {
            return Err(StraitError::MissingConfig(
                "HEMI_TUNNEL_CONTRACT is required".to_string(),
            ));
        }
        if self.ethereum.rpc_url.is_empty() {
            return Err(StraitError::MissingConfig(
                "ETH_RPC_URL is required".to_string(),
            ));
        }
        if self.ethereum.tunnel_contract.is_empty() {
            return Err(StraitError::MissingConfig(
                "ETH_TUNNEL_CONTRACT is required".to_string(),
            ));
        }
        if self.database.url.is_empty() {
            return Err(StraitError::MissingConfig(
                "DATABASE_URL is required".to_string(),
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        assert_eq!(default_bitcoin_confirmation_depth(), 6);
        assert_eq!(default_bitcoin_poll_interval(), 10);
        assert_eq!(default_start_block(), 0);
        assert_eq!(default_evm_confirmation_depth(), 12);
        assert_eq!(default_evm_poll_interval(), 1000);
        assert_eq!(default_max_connections(), 10);
        assert_eq!(default_api_host(), "0.0.0.0");
        assert_eq!(default_api_port(), 8080);
    }

    #[test]
    fn test_config_validation() {
        let config = AppConfig {
            bitcoin: BitcoinConfig {
                rpc_url: String::new(),
                rpc_user: "user".to_string(),
                rpc_password: "pass".to_string(),
                tunnel_addresses: vec![],
                confirmation_depth: 6,
                poll_interval_secs: 10,
            },
            hemi: EvmChainConfig {
                rpc_url: "https://testnet.rpc.hemi.network/rpc".to_string(),
                chain_id: 743111,
                tunnel_contract: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
                bitcoin_kit_contract: Some("0xeC9fa5daC1118963933e1A675a4EEA0009b7f215".to_string()),
                start_block: 0,
                confirmation_depth: 3,
                poll_interval_ms: 1000,
            },
            ethereum: EvmChainConfig {
                rpc_url: "https://eth-sepolia.g.alchemy.com/v2/test".to_string(),
                chain_id: 11155111,
                tunnel_contract: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
                bitcoin_kit_contract: None,
                start_block: 0,
                confirmation_depth: 12,
                poll_interval_ms: 1000,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:password@localhost:5432/strait".to_string(),
                max_connections: 10,
            },
            api: ApiConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
            },
        };

        // Should fail because bitcoin.rpc_url is empty
        assert!(config.validate().is_err());
    }
}
