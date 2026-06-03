//! Strait Core
//!
//! Core domain types, shared traits, errors, and configuration for the Strait tunnel indexer.
//! This crate is the foundation that every other crate depends on.

pub mod config;
pub mod error;
pub mod events;
pub mod types;

// Re-export commonly used types at the crate root
pub use config::AppConfig;
pub use error::{Result, StraitError};
pub use events::{BitcoinEvent, EthereumEvent, HemiEvent, RawEvent};
pub use types::{
    Address, Asset, BitcoinAddress, BitcoinTxid, BlockHash, Chain, ChainAddress, ChainTransaction,
    ChainTxHash, PopAnchor, ReorgEvent, TunnelDirection, TunnelRoute, TunnelStatus, TunnelTransfer,
    TxHash,
};
