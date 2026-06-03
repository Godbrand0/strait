//! Bitcoin tunnel deposit watcher backed by the BitcoinKitV1 precompile.
//!
//! Instead of parsing raw Bitcoin RPC responses for OP_RETURN data, we call
//! BitcoinKitV1 on Hemi directly. This gives us:
//!
//!   - `transactionExists(txId)` — cheap existence check before fetching
//!   - `getTxConfirmations(txId)` — confirmation count without a Bitcoin node
//!   - `getTransactionByTxId(txId)` — full tx including Output.isOpReturn and
//!     Output.opReturnData, which contains the Hemi destination address
//!   - `getUTXOsForBitcoinAddress(addr, page, size)` — watch custody addresses
//!     for new UTXOs instead of scanning every Bitcoin block
//!
//! The Bitcoin RPC node is still used for new-block detection and for
//! computing reorg windows (see `reorg.rs`). BitcoinKit handles verification
//! and data extraction.
//!
//! FIXME: The exact byte encoding of `opReturnData` for Hemi tunnel deposits
//! (i.e. how the Hemi destination address is encoded) must be confirmed with
//! Hemi documentation before the `parse_hemi_destination` function below can
//! be finalised.

use std::collections::HashSet;
use std::sync::Arc;

use alloy::primitives::B256;
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use tracing::{debug, info, warn};

use strait_core::{
    error::{Result, StraitError},
    types::{Address, BitcoinAddress, BitcoinTxid},
};

use crate::contracts::{IBitcoinKitV1, SpentDetail, UTXO, Output, addresses};

// ============================================================================
// BitcoinKit caller
// ============================================================================

/// Thin wrapper around the BitcoinKitV1 precompile for deposit verification
/// and OP_RETURN data extraction.
pub struct BitcoinKitCaller {
    provider: Arc<dyn Provider>,
    contract: alloy::primitives::Address,
}

impl BitcoinKitCaller {
    /// Create a caller for mainnet BitcoinKitV1.
    pub fn mainnet(provider: Arc<dyn Provider>) -> Self {
        Self { provider, contract: addresses::HEMI_BITCOIN_KIT_V1 }
    }

    /// Create a caller for Hemi Sepolia (BitcoinKit v0).
    pub fn testnet(provider: Arc<dyn Provider>) -> Self {
        Self { provider, contract: addresses::HEMI_SEPOLIA_BITCOIN_KIT_V0 }
    }

    /// Check whether a Bitcoin txid exists in the chain as seen by Hemi.
    pub async fn transaction_exists(&self, txid: &BitcoinTxid) -> Result<bool> {
        let call = IBitcoinKitV1::transactionExistsCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let decoded = IBitcoinKitV1::transactionExistsCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("transactionExists decode: {e}")))?;
        Ok(decoded.exists)
    }

    /// Get the number of Bitcoin confirmations for a txid.
    pub async fn get_confirmations(&self, txid: &BitcoinTxid) -> Result<u32> {
        let call = IBitcoinKitV1::getTxConfirmationsCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let decoded = IBitcoinKitV1::getTxConfirmationsCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("getTxConfirmations decode: {e}")))?;
        Ok(decoded.confirmations)
    }

    /// Fetch all UTXOs for a Bitcoin custody address (paginated, page size 50).
    pub async fn get_utxos_for_address(
        &self,
        btc_address: &str,
    ) -> Result<Vec<UTXO>> {
        let mut all = Vec::new();
        let page_size: u32 = 50;
        let mut page: u32 = 0;

        loop {
            let call = IBitcoinKitV1::getUTXOsForBitcoinAddressCall {
                btcAddress: btc_address.to_string(),
                pageNumber: page,
                pageSize: page_size,
            };
            let result = self.call(call.abi_encode()).await?;
            let decoded = IBitcoinKitV1::getUTXOsForBitcoinAddressCall::abi_decode_returns(
                &result, false,
            )
            .map_err(|e| StraitError::Parse(format!("getUTXOs decode: {e}")))?;

            let count = decoded._0.len();
            all.extend(decoded._0);

            if count < page_size as usize {
                break;
            }
            page += 1;
        }

        Ok(all)
    }

    /// Read the OP_RETURN payload from a Bitcoin transaction by txid.
    ///
    /// Uses `getTransactionOutputsByTxId` (precompile 0x??) which fetches only
    /// the outputs — more efficient than `getTransactionByTxId` when inputs are
    /// not needed. Iterates outputs looking for `isOpReturn == true` and returns
    /// the raw `opReturnData` bytes from the first match.
    ///
    /// Returns `None` if the transaction has no OP_RETURN output.
    pub async fn get_op_return_data(&self, txid: &BitcoinTxid) -> Result<Option<Vec<u8>>> {
        let call = IBitcoinKitV1::getTransactionOutputsByTxIdCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let decoded = IBitcoinKitV1::getTransactionOutputsByTxIdCall::abi_decode_returns(
            &result, false,
        )
        .map_err(|e| StraitError::Parse(format!("getTransactionOutputsByTxId decode: {e}")))?;

        for output in decoded._0 {
            if output.isOpReturn {
                return Ok(Some(output.opReturnData.to_vec()));
            }
        }
        Ok(None)
    }

    /// Execute a raw eth_call against the BitcoinKit precompile.
    async fn call(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        use alloy::rpc::types::TransactionRequest;

        let req = TransactionRequest::default()
            .to(self.contract)
            .input(data.into());

        let result = self.provider
            .call(&req)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("BitcoinKit call failed: {e}")))?;

        Ok(result.to_vec())
    }
}

// ============================================================================
// OP_RETURN decoder
// ============================================================================

/// Attempt to parse a Hemi EVM destination address from raw OP_RETURN bytes.
///
/// FIXME: The exact encoding used by the Hemi tunnel to embed the destination
/// address in OP_RETURN payloads has not yet been confirmed from Hemi docs.
/// This function implements two common patterns and should be validated against
/// real testnet transactions before use in production.
///
/// Common patterns:
///   1. Raw 20-byte EVM address (most compact)
///   2. 20-byte address preceded by a 1-byte version/type prefix
pub fn parse_hemi_destination(op_return_data: &[u8]) -> Option<Address> {
    match op_return_data.len() {
        // Pattern 1: raw 20-byte EVM address
        20 => {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(op_return_data);
            Some(Address(addr))
        }
        // Pattern 2: 1-byte prefix + 20-byte address
        21 => {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&op_return_data[1..]);
            Some(Address(addr))
        }
        // Pattern 3: standard ABI-encoded address (32 bytes, right-padded)
        32 => {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&op_return_data[12..32]);
            Some(Address(addr))
        }
        n => {
            warn!(bytes = n, "Unexpected OP_RETURN length — cannot parse Hemi destination");
            None
        }
    }
}

// ============================================================================
// Custody address watcher
// ============================================================================

/// Watches a set of Bitcoin tunnel custody addresses for new UTXOs using
/// BitcoinKitV1, emitting deposit candidates for the ingester to process.
pub struct CustodyWatcher {
    caller: BitcoinKitCaller,
    addresses: HashSet<String>,
    /// Txids already processed — prevents re-emitting on subsequent polls.
    seen: HashSet<[u8; 32]>,
}

impl CustodyWatcher {
    pub fn new(caller: BitcoinKitCaller, addresses: Vec<String>) -> Self {
        Self {
            caller,
            addresses: addresses.into_iter().collect(),
            seen: HashSet::new(),
        }
    }

    /// Poll all watched addresses and return any new UTXOs not yet seen.
    pub async fn poll_new_deposits(&mut self) -> Result<Vec<DepositCandidate>> {
        let mut candidates = Vec::new();

        for addr in &self.addresses.clone() {
            let utxos = self.caller.get_utxos_for_address(addr).await?;
            debug!(address = %addr, count = utxos.len(), "Polled UTXOs");

            for utxo in utxos {
                let txid: [u8; 32] = utxo.txId.into();

                if self.seen.contains(&txid) {
                    continue;
                }

                // Fetch OP_RETURN data to extract the Hemi destination
                let bitcoin_txid = BitcoinTxid(txid);
                let op_return = self.caller.get_op_return_data(&bitcoin_txid).await?;

                let hemi_destination = op_return.as_deref().and_then(parse_hemi_destination);

                if hemi_destination.is_none() {
                    warn!(
                        txid = %hex::encode(txid),
                        "No parseable OP_RETURN on deposit UTXO — skipping"
                    );
                    self.seen.insert(txid);
                    continue;
                }

                let confirmations = self.caller.get_confirmations(&bitcoin_txid).await?;

                info!(
                    txid = %hex::encode(txid),
                    amount_sats = %utxo.value,
                    confirmations,
                    "New tunnel deposit candidate"
                );

                candidates.push(DepositCandidate {
                    txid: bitcoin_txid,
                    vout: utxo.index.saturating_to::<u32>(),
                    amount_sats: utxo.value.saturating_to::<u64>(),
                    to_address: BitcoinAddress::new(addr.clone()),
                    hemi_destination: hemi_destination.unwrap(),
                    confirmations,
                });

                self.seen.insert(txid);
            }
        }

        Ok(candidates)
    }
}

/// A Bitcoin UTXO deposited to a tunnel custody address with the decoded
/// Hemi destination address from its OP_RETURN output.
#[derive(Debug, Clone)]
pub struct DepositCandidate {
    pub txid: BitcoinTxid,
    pub vout: u32,
    pub amount_sats: u64,
    pub to_address: BitcoinAddress,
    pub hemi_destination: Address,
    pub confirmations: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hemi_destination_20_bytes() {
        let addr_bytes = [0xABu8; 20];
        let result = parse_hemi_destination(&addr_bytes).unwrap();
        assert_eq!(result.0, addr_bytes);
    }

    #[test]
    fn test_parse_hemi_destination_21_bytes_with_prefix() {
        let mut data = vec![0x01u8]; // version prefix
        data.extend_from_slice(&[0xCDu8; 20]);
        let result = parse_hemi_destination(&data).unwrap();
        assert_eq!(result.0, [0xCDu8; 20]);
    }

    #[test]
    fn test_parse_hemi_destination_32_bytes_abi_encoded() {
        let mut data = vec![0u8; 12];
        data.extend_from_slice(&[0xEFu8; 20]);
        let result = parse_hemi_destination(&data).unwrap();
        assert_eq!(result.0, [0xEFu8; 20]);
    }

    #[test]
    fn test_parse_hemi_destination_unknown_length() {
        assert!(parse_hemi_destination(&[0u8; 7]).is_none());
    }
}
