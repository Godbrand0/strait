//! Bitcoin payout watcher — finalizes Hemi→BTC withdrawals once their Bitcoin
//! payout is observed.
//!
//! Withdrawals have no Hemi-side "fulfilled" event: the operator sends BTC to the
//! user's address out on Bitcoin. We detect that payout by watching the
//! withdrawal's recipient address through BitcoinKit on Hemi (no Bitcoin node
//! required) for a UTXO matching the net amount, then mark the transfer FINALIZED.
//!
//! Best-effort by design:
//!   - BitcoinKit exposes *unspent* outputs, so a payout already spent by the
//!     recipient before we poll cannot be observed. Frequent polling catches live
//!     payouts; already-spent historical ones are missed.
//!   - Matching is by recipient + amount (within a small tolerance). The uuid in
//!     the payout's OP_RETURN would be exact once its encoding is confirmed — we log
//!     any OP_RETURN found on a matched payout to help confirm that encoding.

use std::time::Duration;

use bigdecimal::ToPrimitive;
use chrono::Utc;
use tokio::time::sleep;
use tracing::{info, warn};

use strait_bitcoin::BitcoinKitCaller;
use strait_core::error::Result;
use strait_core::types::BitcoinTxid;
use strait_store::{Database, TunnelTransferRepo};

/// Polls pending Hemi→BTC withdrawals and finalizes each one whose Bitcoin payout
/// is observed via BitcoinKit.
pub struct BtcPayoutWatcher {
    caller: BitcoinKitCaller,
    db: Database,
    poll_interval: Duration,
    min_confirmations: u32,
}

impl BtcPayoutWatcher {
    pub fn new(
        caller: BitcoinKitCaller,
        db: Database,
        poll_interval_secs: u64,
        min_confirmations: u32,
    ) -> Self {
        Self {
            caller,
            db,
            // Payouts confirm over minutes — polling faster just burdens the Hemi RPC.
            poll_interval: Duration::from_secs(poll_interval_secs.max(60)),
            min_confirmations,
        }
    }

    pub async fn run(self) -> Result<()> {
        info!(
            min_confirmations = self.min_confirmations,
            poll_secs = self.poll_interval.as_secs(),
            "BTC payout watcher started"
        );
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "BTC payout watcher tick failed");
            }
            sleep(self.poll_interval).await;
        }
    }

    async fn tick(&self) -> Result<()> {
        let repo = TunnelTransferRepo::new(&self.db);
        let pending = repo.list_pending_btc_payouts(200).await?;
        for w in pending {
            if !is_real_btc_address(&w.recipient) {
                continue; // placeholder recipient — nothing to watch yet
            }
            let want_sats = match w.amount.to_u64() {
                Some(s) if s > 0 => s,
                _ => continue,
            };

            let utxos = match self.caller.get_utxos_for_address(&w.recipient).await {
                Ok(u) => u,
                Err(e) => {
                    warn!(recipient = %w.recipient, error = %e, "UTXO lookup failed — skipping");
                    continue;
                }
            };

            for utxo in utxos {
                let value = utxo.value.saturating_to::<u64>();
                if !amount_matches(value, want_sats) {
                    continue;
                }
                let txid = BitcoinTxid(utxo.txId.into());
                let confs = self.caller.get_confirmations(&txid).await.unwrap_or(0);
                if confs < self.min_confirmations {
                    continue; // payout seen but not yet final
                }

                // Log any OP_RETURN on the payout — helps confirm the uuid encoding.
                if let Ok(Some(op)) = self.caller.get_op_return_data(&txid).await {
                    info!(txid = %hex::encode(txid.0), op_return = %hex::encode(&op), "payout OP_RETURN");
                }

                let txid_hex = hex::encode(txid.0);
                repo.set_btc_payout(w.id, &txid_hex, None, Utc::now()).await?;
                info!(
                    transfer = %w.id,
                    recipient = %w.recipient,
                    sats = value,
                    confirmations = confs,
                    txid = %txid_hex,
                    "Hemi→BTC withdrawal FINALIZED — Bitcoin payout observed"
                );
                break;
            }
        }
        Ok(())
    }
}

/// True for a real Bitcoin address — i.e. not the `withdrawal-uuid-…` placeholder
/// that's stored when the recipient couldn't be recovered from the calldata.
fn is_real_btc_address(s: &str) -> bool {
    if s.starts_with("withdrawal-uuid-") {
        return false;
    }
    // bech32 (bc1/tb1) or base58 (1/3 mainnet, m/n/2 testnet) prefixes.
    matches!(s.chars().next(), Some('b' | 't' | '1' | '3' | 'm' | 'n' | '2'))
}

/// Match a payout UTXO value to the withdrawal's net sats, tolerating a small
/// Bitcoin-fee difference (≤1% or 1000 sats, whichever is larger).
fn amount_matches(utxo_sats: u64, net_sats: u64) -> bool {
    if utxo_sats == 0 || net_sats == 0 {
        return false;
    }
    let tol = (net_sats / 100).max(1000);
    utxo_sats.abs_diff(net_sats) <= tol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_near_amounts_match() {
        assert!(amount_matches(598_800, 598_800));        // exact
        assert!(amount_matches(598_000, 598_800));        // within 1000 sats
        assert!(!amount_matches(500_000, 598_800));       // too far off
        assert!(!amount_matches(0, 598_800));             // zero never matches
    }

    #[test]
    fn placeholder_recipients_are_skipped() {
        assert!(!is_real_btc_address("withdrawal-uuid-804583"));
        assert!(is_real_btc_address("bc1qwql2auj2u56sk87n2p8g464nnswp6qarkcy5tk"));
        assert!(is_real_btc_address("tb1qexampletestnetaddr"));
    }
}
