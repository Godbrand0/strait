//! EVM chain ingester for processing Hemi tunnel contract events.
//!
//! Watches for TunnelIn, TunnelOut, TunnelOutComplete, and PoP events
//! from the Hemi tunnel contracts and converts them to raw events.

use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::{Address as AlloyAddress, B256, U256};
use alloy::providers::Provider;
use alloy::rpc::types::Log;
use alloy::sol_types::SolEvent;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};

use strait_core::{
    config::EvmChainConfig,
    error::{Result, StraitError},
    events::{RawEvent, HemiEvent},
    types::{Address, Asset, BitcoinTxid, ChainAddress, BitcoinAddress, TxHash},
};

use crate::contracts::{IBitcoinTunnelManager, IStandardBridge, topics};
use crate::reorg::ReorgDetector;

/// EVM chain ingester that watches for tunnel contract events.
pub struct EvmIngester {
    config: EvmChainConfig,
    provider: Arc<dyn Provider>,
    reorg_detector: ReorgDetector,
    event_tx: mpsc::Sender<RawEvent>,
}

impl EvmIngester {
    /// Create a new EVM ingester.
    pub fn new(
        config: EvmChainConfig,
        provider: Arc<dyn Provider>,
        event_tx: mpsc::Sender<RawEvent>,
    ) -> Self {
        let reorg_detector = ReorgDetector::new(config.confirmation_depth as u64);
        
        Self {
            config,
            provider,
            reorg_detector,
            event_tx,
        }
    }

    /// Run the ingester, watching for new blocks and processing events.
    #[instrument(skip(self), fields(chain = %self.config.chain_id))]
    pub async fn run(self) -> Result<()> {
        info!("Starting EVM ingester for chain {}", self.config.chain_id);
        
        let mut last_block = self.get_start_block().await?;
        info!("Starting from block {}", last_block);
        
        loop {
            match self.process_new_blocks(&last_block).await {
                Ok(new_last) => {
                    last_block = new_last;
                }
                Err(e) => {
                    error!("Error processing blocks: {}", e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
            
            sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
        }
    }

    /// Get the starting block height.
    async fn get_start_block(&self) -> Result<u64> {
        let latest = self.provider
            .get_block_number()
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get block number: {}", e)))?;
        
        // Start from confirmation_depth blocks ago
        Ok(latest.saturating_sub(self.config.confirmation_depth as u64))
    }

    /// Process new blocks and emit events.
    async fn process_new_blocks(&self, last_block: &u64) -> Result<u64> {
        let latest = self.provider
            .get_block_number()
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get block number: {}", e)))?;
        
        let confirmed = latest.saturating_sub(self.config.confirmation_depth as u64);
        
        if confirmed <= *last_block {
            debug!("No new confirmed blocks (latest={}, confirmed={})", latest, confirmed);
            return Ok(*last_block);
        }
        
        // Check for reorgs using the detector
        if self.reorg_detector.detect_reorg(&*self.provider, *last_block).await? {
            warn!("Reorg detected at block {}", *last_block);
            // Reorg detected - caller should handle by rolling back
            return Err(StraitError::Chain(format!("Reorg detected at block {}", *last_block)));
        }
        
        // Process blocks one at a time
        let mut new_last = *last_block;
        for block_num in (*last_block + 1)..=confirmed {
            self.process_block(block_num).await?;
            new_last = block_num;
        }
        
        Ok(new_last)
    }

    /// Process a single block for tunnel events.
    #[instrument(skip(self), fields(block = block_num))]
    async fn process_block(&self, block_num: u64) -> Result<()> {
        debug!("Processing block {}", block_num);
        
        // Get block with timestamp
        let block = self.provider
            .get_block_by_number(block_num.into(), false)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get block: {}", e)))?
            .ok_or_else(|| StraitError::EvmProvider(format!("Block {} not found", block_num)))?;
        
        let block_hash = block.header.hash;
        let block_time = DateTime::from_timestamp(block.header.timestamp as i64, 0)
            .unwrap_or_else(Utc::now);
        
        // Get logs for tunnel contract
        let logs = self.get_tunnel_logs(block_num).await?;
        
        for log in logs {
            self.process_log(log, block_num, block_hash, block_time).await?;
        }
        
        Ok(())
    }

    /// Get logs from the tunnel contract for a specific block.
    async fn get_tunnel_logs(&self, block_num: u64) -> Result<Vec<Log>> {
        // Parse the tunnel contract address from hex string
        let tunnel_address: AlloyAddress = self.config.tunnel_contract.parse()
            .map_err(|e| StraitError::Parse(format!("Invalid tunnel contract address: {}", e)))?;
        
        let filter = alloy::rpc::types::Filter::new()
            .address(tunnel_address)
            .from_block(block_num)
            .to_block(block_num);
        
        let logs = self.provider
            .get_logs(&filter)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get logs: {}", e)))?;
        
        Ok(logs)
    }

    /// Process a single log entry.
    ///
    /// Dispatches to either the OP Stack StandardBridge handlers (ETH/ERC-20
    /// routes) or the BTC tunnel handlers depending on the event topic.
    async fn process_log(
        &self,
        log: Log,
        block_num: u64,
        _block_hash: B256,
        _block_time: DateTime<Utc>,
    ) -> Result<()> {
        let tx_hash = log.transaction_hash
            .ok_or_else(|| StraitError::Parse("Missing transaction hash".into()))?;

        let log_index = log.log_index.unwrap_or(0) as u32;
        let log_topics: Vec<B256> = log.topics().to_vec();
        let data = &log.data().data;

        if log_topics.is_empty() {
            return Ok(());
        }

        let topic0 = log_topics[0];

        // ── OP Stack StandardBridge events (ETH / ERC-20 routes) ─────────────
        //
        // alloy-sol-types 0.3 exposes `SolEvent::decode_data(data, validate)`
        // which decodes only the non-indexed portion of a log. Indexed fields
        // are extracted manually from the topics array.

        if topic0 == topics::eth_bridge_finalized() {
            // ETHBridgeFinalized(indexed from, indexed to, uint256 amount, bytes extraData)
            if let (Some(from_t), Some(to_t)) = (log_topics.get(1), log_topics.get(2)) {
                let from = addr_from_topic(from_t);
                let to   = addr_from_topic(to_t);
                if let Ok(decoded) = IStandardBridge::ETHBridgeFinalized::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_eth_deposit(from, to, decoded.amount, tx_hash, block_num, log_index).await?;
                }
            }
        } else if topic0 == topics::eth_bridge_initiated() {
            // ETHBridgeInitiated(indexed from, indexed to, uint256 amount, bytes extraData)
            if let (Some(from_t), Some(to_t)) = (log_topics.get(1), log_topics.get(2)) {
                let from = addr_from_topic(from_t);
                let to   = addr_from_topic(to_t);
                if let Ok(decoded) = IStandardBridge::ETHBridgeInitiated::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_eth_withdrawal(from, to, decoded.amount, tx_hash, block_num, log_index).await?;
                }
            }
        } else if topic0 == topics::erc20_bridge_finalized() {
            // ERC20BridgeFinalized(indexed localToken, indexed remoteToken, indexed from,
            //                      address to, uint256 amount, bytes extraData)
            if let (Some(lt), Some(rt), Some(ft)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let local_token  = addr_from_topic(lt);
                let remote_token = addr_from_topic(rt);
                let from         = addr_from_topic(ft);
                if let Ok(decoded) = IStandardBridge::ERC20BridgeFinalized::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    let to = Address(decoded.to.into());
                    self.handle_erc20_deposit(
                        local_token, remote_token, from, to,
                        decoded.amount, tx_hash, block_num, log_index,
                    ).await?;
                }
            }
        } else if topic0 == topics::erc20_bridge_initiated() {
            // ERC20BridgeInitiated(indexed localToken, indexed remoteToken, indexed from,
            //                      address to, uint256 amount, bytes extraData)
            if let (Some(lt), Some(rt), Some(ft)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let local_token  = addr_from_topic(lt);
                let remote_token = addr_from_topic(rt);
                let from         = addr_from_topic(ft);
                if let Ok(decoded) = IStandardBridge::ERC20BridgeInitiated::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    let to = Address(decoded.to.into());
                    self.handle_erc20_withdrawal(
                        local_token, remote_token, from, to,
                        decoded.amount, tx_hash, block_num, log_index,
                    ).await?;
                }
            }

        // ── BitcoinTunnelManager events (BTC routes) ──────────────────────────
        // Confirmed from hemilabs/bitcoin-tunnel-contracts source.

        } else if topic0 == topics::deposit_confirmed() {
            // DepositConfirmed(indexed vault, indexed recipient, indexed depositTxId,
            //                  uint256 depositSats, uint256 netSatsAfterFee)
            // depositTxId is the Bitcoin txid — the primary cross-chain join key.
            if let (Some(vault_t), Some(recipient_t), Some(txid_t)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let recipient = addr_from_topic(recipient_t);
                let deposit_tx_id: [u8; 32] = txid_t.0;
                if let Ok(decoded) = IBitcoinTunnelManager::DepositConfirmed::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_btc_deposit_confirmed(
                        addr_from_topic(vault_t), recipient, deposit_tx_id,
                        decoded.netSatsAfterFee, tx_hash, block_num, log_index,
                    ).await?;
                }
            }
        } else if topic0 == topics::btc_withdrawal_initiated() {
            // WithdrawalInitiated(indexed vault, indexed withdrawer, indexed btcAddress,
            //                     uint256 withdrawalSats, uint256 netSatsAfterFee, uint64 uuid)
            // Note: btcAddress is indexed so it is keccak256-hashed in topics[3] —
            // the original string cannot be recovered from the log. The uuid in the
            // non-indexed data is the correct cross-chain join key for this withdrawal.
            if let Some(withdrawer_t) = log_topics.get(2) {
                let withdrawer = addr_from_topic(withdrawer_t);
                if let Ok(decoded) = IBitcoinTunnelManager::WithdrawalInitiated::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_btc_withdrawal_initiated(
                        withdrawer, decoded.netSatsAfterFee, decoded.uuid,
                        tx_hash, block_num, log_index,
                    ).await?;
                }
            }
        } else if topic0 == topics::vault_created() {
            // VaultCreated(indexed setupAdmin, indexed operatorAdmin, indexed vaultAddress)
            // Track new vaults so the CustodyWatcher can watch their Bitcoin addresses.
            if let Some(vault_t) = log_topics.get(3) {
                let vault_address = addr_from_topic(vault_t);
                info!(vault = %hex::encode(vault_address.0), "New BTC tunnel vault created — add to watched addresses");
            }
        } else {
            debug!("Unknown event topic {:?}, skipping", topic0);
        }

        Ok(())
    }

    // ── StandardBridge handlers (ETH / ERC-20 routes) ────────────────────────

    async fn handle_eth_deposit(
        &self,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
    ) -> Result<()> {
        info!(from = %hex::encode(from.0), to = %hex::encode(to.0), %amount, "ETHBridgeFinalized (deposit on Hemi)");
        let event = RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Eth,
            amount: u256_to_bigdecimal(amount)?,
            to,
            source_txid: None,
            block_number: block_num,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    async fn handle_eth_withdrawal(
        &self,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
    ) -> Result<()> {
        info!(from = %hex::encode(from.0), to = %hex::encode(to.0), %amount, "ETHBridgeInitiated (withdrawal from Hemi)");
        let event = RawEvent::Hemi(HemiEvent::TunnelBurn {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Eth,
            amount: u256_to_bigdecimal(amount)?,
            from,
            destination: ChainAddress::Evm(to),
            block_number: block_num,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    async fn handle_erc20_deposit(
        &self,
        local_token: Address,
        _remote_token: Address,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
    ) -> Result<()> {
        info!(
            token = %hex::encode(local_token.0),
            from = %hex::encode(from.0),
            to = %hex::encode(to.0),
            %amount,
            "ERC20BridgeFinalized (deposit on Hemi)"
        );
        let event = RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Erc20 {
                contract: local_token,
                symbol: String::new(),  // resolved off the token-list if needed
                decimals: 18,
            },
            amount: u256_to_bigdecimal(amount)?,
            to,
            source_txid: None,
            block_number: block_num,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    async fn handle_erc20_withdrawal(
        &self,
        local_token: Address,
        _remote_token: Address,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
    ) -> Result<()> {
        info!(
            token = %hex::encode(local_token.0),
            from = %hex::encode(from.0),
            to = %hex::encode(to.0),
            %amount,
            "ERC20BridgeInitiated (withdrawal from Hemi)"
        );
        let event = RawEvent::Hemi(HemiEvent::TunnelBurn {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Erc20 {
                contract: local_token,
                symbol: String::new(),
                decimals: 18,
            },
            amount: u256_to_bigdecimal(amount)?,
            from,
            destination: ChainAddress::Evm(to),
            block_number: block_num,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    // ── BitcoinTunnelManager handlers (BTC routes) ────────────────────────────

    /// Handle DepositConfirmed (BTC → Hemi): Bitcoin deposit confirmed, hBTC minted.
    /// `deposit_tx_id` is the Bitcoin txid — primary cross-chain join key.
    async fn handle_btc_deposit_confirmed(
        &self,
        _vault: Address,
        recipient: Address,
        deposit_tx_id: [u8; 32],
        net_sats: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
    ) -> Result<()> {
        info!(
            recipient = %hex::encode(recipient.0),
            bitcoin_txid = %hex::encode(deposit_tx_id),
            net_sats = %net_sats,
            "DepositConfirmed — BTC deposited and hBTC minted on Hemi"
        );
        let event = RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Btc,
            amount: u256_to_bigdecimal(net_sats)?,
            to: recipient,
            source_txid: Some(BitcoinTxid(deposit_tx_id)),
            block_number: block_num,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    /// Handle WithdrawalInitiated (Hemi → BTC): hBTC burned, BTC payout pending.
    ///
    /// The `btcAddress` field in the event is `indexed`, so it is keccak256-hashed
    /// in the topic and the original string is unrecoverable from the log. The `uuid`
    /// (vaultIndex << 32 | vaultSpecificUUID) is the cross-chain join key: the
    /// Bitcoin payout transaction includes an OP_RETURN encoding this uuid.
    async fn handle_btc_withdrawal_initiated(
        &self,
        withdrawer: Address,
        net_sats: U256,
        uuid: u64,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
    ) -> Result<()> {
        info!(
            withdrawer = %hex::encode(withdrawer.0),
            %net_sats,
            uuid,
            "WithdrawalInitiated — hBTC burned, BTC payout registered"
        );
        let event = RawEvent::Hemi(HemiEvent::TunnelBurn {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Btc,
            amount: u256_to_bigdecimal(net_sats)?,
            from: withdrawer,
            // btcAddress is not recoverable from an indexed string topic.
            // The bitcoin address will be resolved by watching the payout tx on Bitcoin.
            destination: ChainAddress::Bitcoin(BitcoinAddress::new(format!("withdrawal-uuid-{uuid}"))),
            block_number: block_num,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }
}

/// Extract an EVM address from a 32-byte log topic (right-padded to 32 bytes).
fn addr_from_topic(topic: &alloy::primitives::B256) -> Address {
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&topic.0[12..32]);
    Address(addr)
}

/// Convert U256 to BigDecimal.
fn u256_to_bigdecimal(value: U256) -> Result<BigDecimal> {
    let s = value.to_string();
    s.parse::<BigDecimal>()
        .map_err(|e| StraitError::Parse(format!("Failed to parse U256 as BigDecimal: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_u256_to_bigdecimal() {
        let value = U256::from(100000000u64); // 1 BTC in satoshis
        let bd = u256_to_bigdecimal(value).unwrap();
        assert_eq!(bd, BigDecimal::from(100000000u64));
    }
}