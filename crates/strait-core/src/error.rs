//! Error types for Strait.
//!
//! Uses `thiserror` for library errors and provides a unified error type
//! that other crates can use.

use std::num::ParseIntError;

/// The main error type for Strait operations
#[derive(Debug, thiserror::Error)]
pub enum StraitError {
    // ============================================================================
    // Configuration errors
    // ============================================================================
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    // ============================================================================
    // Chain connection errors
    // ============================================================================
    #[error("Bitcoin RPC error: {0}")]
    BitcoinRpc(String),

    #[error("EVM provider error: {0}")]
    EvmProvider(String),

    #[error("Chain not connected: {0}")]
    ChainNotConnected(String),

    #[error("Chain error: {0}")]
    Chain(String),

    // ============================================================================
    // Ingestion errors
    // ============================================================================
    #[error("Block not found: {chain} block {block_number}")]
    BlockNotFound { chain: String, block_number: u64 },

    #[error("Transaction not found: {chain} tx {tx_hash}")]
    TransactionNotFound { chain: String, tx_hash: String },

    #[error("Reorg detected on {chain} at block {block_number}, depth {depth}")]
    ReorgDetected {
        chain: String,
        block_number: u64,
        depth: u32,
    },

    #[error("Ingestion lag: {chain} is {lag_blocks} blocks behind")]
    IngestionLag { chain: String, lag_blocks: u64 },

    // ============================================================================
    // Join engine errors
    // ============================================================================
    #[error("Transfer not found: {0}")]
    TransferNotFound(uuid::Uuid),

    #[error("Invalid state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("Event matching failed: {0}")]
    EventMatchingFailed(String),

    // ============================================================================
    // Database errors
    // ============================================================================
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(String),

    // ============================================================================
    // Serialization errors
    // ============================================================================
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Hex decoding error: {0}")]
    HexDecode(#[from] hex::FromHexError),

    #[error("Invalid hex length: expected {expected}, got {actual}")]
    InvalidHexLength { expected: usize, actual: usize },

    // ============================================================================
    // HTTP/API errors
    // ============================================================================
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Webhook delivery failed: {url} - {reason}")]
    WebhookDeliveryFailed { url: String, reason: String },

    #[error("GraphQL error: {0}")]
    GraphQL(String),

    // ============================================================================
    // Parsing errors
    // ============================================================================
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    // ============================================================================
    // Internal errors
    // ============================================================================
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Channel closed: {0}")]
    ChannelClosed(String),

    #[error("Task panicked: {0}")]
    TaskPanicked(String),
}

/// Result type alias for Strait operations
pub type Result<T> = std::result::Result<T, StraitError>;

// ============================================================================
// Conversions from external error types
// ============================================================================

impl From<ParseIntError> for StraitError {
    fn from(err: ParseIntError) -> Self {
        Self::Parse(err.to_string())
    }
}

impl From<reqwest::Error> for StraitError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err.to_string())
    }
}

impl From<anyhow::Error> for StraitError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

impl From<config::ConfigError> for StraitError {
    fn from(err: config::ConfigError) -> Self {
        Self::Config(err.to_string())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StraitError::Config("missing RPC URL".to_string());
        assert_eq!(err.to_string(), "Configuration error: missing RPC URL");

        let err = StraitError::BlockNotFound {
            chain: "Bitcoin".to_string(),
            block_number: 12345,
        };
        assert_eq!(err.to_string(), "Block not found: Bitcoin block 12345");

        let err = StraitError::ReorgDetected {
            chain: "Bitcoin".to_string(),
            block_number: 100,
            depth: 2,
        };
        assert_eq!(
            err.to_string(),
            "Reorg detected on Bitcoin at block 100, depth 2"
        );
    }

    #[test]
    fn test_error_from_hex() {
        let hex_err = hex::FromHexError::InvalidStringLength;
        let err: StraitError = hex_err.into();
        assert!(matches!(err, StraitError::HexDecode(_)));
    }
}
