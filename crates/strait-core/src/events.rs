//! Raw ingestion events emitted by chain ingesters before the join engine processes them.
//!
//! These events are separate from domain types to maintain a clean boundary between
//! ingestion and join layers.

use bigdecimal::BigDecimal;
use bitcoin::Txid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{Address, Asset, BitcoinAddress, BitcoinTxid, BlockHash, ChainAddress, TxHash};

/// Chain identifier for multi-chain support.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChainId {
    Bitcoin,
    Evm(u64),
}

/// Block reference with height, hash, and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRef {
    pub height: u64,
    pub hash: String,
    pub timestamp: u64,
}

/// Direction of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    BitcoinToEvm,
    EvmToBitcoin,
}

/// A transfer event representing value movement between chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub direction: TransferDirection,
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    pub source_txid: String,
    pub dest_txid: String,
    pub log_index: u64,
    pub metadata: serde_json::Value,
}

/// Withdrawal completion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalComplete {
    pub nonce: u64,
    pub bitcoin_txid: String,
    pub evm_txid: String,
}

/// Type of event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    Transfer(Transfer),
    WithdrawalComplete(WithdrawalComplete),
}

/// A chain event with full context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEvent {
    pub chain_id: ChainId,
    pub block: BlockRef,
    pub txid: Txid,
    pub kind: EventKind,
}

/// Raw event from any chain ingester
#[derive(Debug, Clone)]
pub enum RawEvent {
    Bitcoin(BitcoinEvent),
    Hemi(HemiEvent),
    Ethereum(EthereumEvent),
}

/// Events from the Bitcoin chain ingester
#[derive(Debug, Clone)]
pub enum BitcoinEvent {
    /// A deposit to a tunnel custody address
    TunnelDeposit {
        txid: BitcoinTxid,
        vout: u32,
        to_address: BitcoinAddress,
        amount_sats: u64,
        op_return_data: Option<Vec<u8>>,
        block_number: u64,
        block_hash: BlockHash,
        block_time: DateTime<Utc>,
    },
    /// A withdrawal spending a previously-seen tunnel UTXO
    TunnelWithdrawal {
        txid: BitcoinTxid,
        from_address: BitcoinAddress,
        to_address: BitcoinAddress,
        amount_sats: u64,
        block_number: u64,
        block_hash: BlockHash,
        block_time: DateTime<Utc>,
    },
    /// A chain reorganization was detected
    BlockReorg {
        old_tip: BlockHash,
        new_tip: BlockHash,
        depth: u32,
        affected_from_block: u64,
    },
}

/// Events from the Hemi EVM chain ingester
#[derive(Debug, Clone)]
pub enum HemiEvent {
    /// A tunnel mint (BTC or ETH deposited into Hemi)
    TunnelMint {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        to: Address,
        /// Present for BTC routes — links to the Bitcoin deposit
        source_txid: Option<BitcoinTxid>,
        block_number: u64,
        log_index: u32,
    },
    /// A tunnel burn (assets being withdrawn from Hemi)
    TunnelBurn {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        from: Address,
        destination: ChainAddress,
        block_number: u64,
        log_index: u32,
    },
    /// A PoP proof submission anchoring Hemi blocks to Bitcoin
    PopProofSubmitted {
        tx_hash: TxHash,
        bitcoin_txid: BitcoinTxid,
        hemi_block_range: (u64, u64),
        block_number: u64,
    },
    /// A chain reorganization was detected
    BlockReorg {
        old_tip: BlockHash,
        new_tip: BlockHash,
        depth: u32,
        affected_from_block: u64,
    },
}

/// Events from the Ethereum chain ingester
#[derive(Debug, Clone)]
pub enum EthereumEvent {
    /// A tunnel lock (ETH or ERC-20 locked for tunneling to Hemi)
    TunnelLock {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        from: Address,
        block_number: u64,
        log_index: u32,
    },
    /// A tunnel release (assets released from Hemi to Ethereum)
    TunnelRelease {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        to: Address,
        block_number: u64,
        log_index: u32,
    },
    /// A chain reorganization was detected
    BlockReorg {
        old_tip: BlockHash,
        new_tip: BlockHash,
        depth: u32,
        affected_from_block: u64,
    },
}

impl RawEvent {
    /// Get the chain this event originated from
    pub fn chain(&self) -> crate::types::Chain {
        match self {
            Self::Bitcoin(_) => crate::types::Chain::Bitcoin,
            Self::Hemi(_) => crate::types::Chain::Hemi,
            Self::Ethereum(_) => crate::types::Chain::Ethereum,
        }
    }

    /// Check if this is a reorg event
    pub fn is_reorg(&self) -> bool {
        matches!(
            self,
            Self::Bitcoin(BitcoinEvent::BlockReorg { .. })
                | Self::Hemi(HemiEvent::BlockReorg { .. })
                | Self::Ethereum(EthereumEvent::BlockReorg { .. })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_event_chain() {
        let btc_event = RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
            txid: BitcoinTxid([0; 32]),
            vout: 0,
            to_address: BitcoinAddress::new("bc1qtest"),
            amount_sats: 100000000,
            op_return_data: None,
            block_number: 100,
            block_hash: BlockHash([0; 32]),
            block_time: Utc::now(),
        });
        assert_eq!(btc_event.chain(), crate::types::Chain::Bitcoin);
        assert!(!btc_event.is_reorg());

        let reorg_event = RawEvent::Bitcoin(BitcoinEvent::BlockReorg {
            old_tip: BlockHash([0; 32]),
            new_tip: BlockHash([1; 32]),
            depth: 2,
            affected_from_block: 98,
        });
        assert!(reorg_event.is_reorg());
    }
}
