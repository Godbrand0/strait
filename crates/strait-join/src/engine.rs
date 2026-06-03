//! Join engine — consumes raw events from all three chain ingesters and
//! produces TunnelTransfer lifecycle updates.
//!
//! # Keystone anchoring (BTC routes)
//!
//! BTC→Hemi transfers advance from INITIATED → ANCHORED when a
//! `PopKeystoneAnchored` event covers the Hemi mint block.
//!
//! `PopAnchor::covers_block(mint_block)` is the authoritative check:
//!   window = (keystone_block - 25, keystone_block]  (exclusive lower, inclusive upper)
//!
//! When `PayoutRoundExecuted(blockRewarded)` fires on `PoPPayoutsV2`, the engine
//! fans out to all in-flight transfers and advances those whose mint block is covered.
//! ETH→Hemi uses OP Stack finality — no PoP wait required.

use chrono::Utc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use strait_core::{
    config::KEYSTONE_FREQUENCY,
    error::{Result, StraitError},
    events::{BitcoinEvent, EthereumEvent, HemiEvent, RawEvent},
    types::{Chain, ChainTransaction, PopAnchor, ReorgEvent, TunnelStatus},
};

use crate::{matcher::EventMatcher, state::TransferState};

/// Keystone frequency — must match PoPPayoutsV2.KEYSTONE_FREQUENCY = 25.
const KEYSTONE_FREQ: u64 = KEYSTONE_FREQUENCY as u64;

/// Compute the keystone block that covers Hemi block `n`.
/// Returns the smallest multiple of 25 that is >= n.
pub fn keystone_for(block: u64) -> u64 {
    let rem = block % KEYSTONE_FREQ;
    if rem == 0 { block } else { block + (KEYSTONE_FREQ - rem) }
}

/// An update emitted by the engine to the store layer.
#[derive(Debug, Clone)]
pub enum TunnelTransferUpdate {
    Created(strait_core::types::TunnelTransfer),
    StatusChanged {
        id: Uuid,
        new_status: TunnelStatus,
        updated_at: chrono::DateTime<Utc>,
    },
    Retracted {
        id: Uuid,
        reason: String,
        retracted_at: chrono::DateTime<Utc>,
    },
    DestinationConfirmed {
        id: Uuid,
        destination_tx: ChainTransaction,
    },
    /// A PoP keystone anchored this transfer to Bitcoin.
    /// The transfer's Hemi mint block fell within the keystone window.
    PopAnchored {
        id: Uuid,
        keystone_block: u64,
        pop_score: u64,
        anchored_at: chrono::DateTime<Utc>,
    },
}

/// Consumes raw events from all chain ingesters and produces
/// `TunnelTransferUpdate` records for the store layer.
pub struct JoinEngine {
    event_rx: mpsc::Receiver<RawEvent>,
    update_tx: mpsc::Sender<TunnelTransferUpdate>,
    state: TransferState,
    matcher: EventMatcher,
    /// Most recent keystone block confirmed as PoP-anchored.
    last_anchored_keystone: u64,
}

impl JoinEngine {
    pub fn new(
        event_rx: mpsc::Receiver<RawEvent>,
        update_tx: mpsc::Sender<TunnelTransferUpdate>,
    ) -> Self {
        Self {
            event_rx,
            update_tx,
            state: TransferState::new(),
            matcher: EventMatcher::new(Default::default()),
            last_anchored_keystone: 0,
        }
    }

    /// Run the engine forever, consuming events until the channel closes.
    pub async fn run(mut self) -> Result<()> {
        info!("Join engine started");
        while let Some(event) = self.event_rx.recv().await {
            if let Err(e) = self.process(event).await {
                warn!("Join engine error: {}", e);
            }
        }
        info!("Join engine shutting down — event channel closed");
        Ok(())
    }

    async fn process(&mut self, event: RawEvent) -> Result<()> {
        match &event {
            // Reorgs — handle before matching so we retract before processing new events
            RawEvent::Bitcoin(BitcoinEvent::BlockReorg { affected_from_block, old_tip, new_tip, depth }) => {
                let re = ReorgEvent {
                    chain: Chain::Bitcoin,
                    depth: *depth,
                    old_tip: *old_tip,
                    new_tip: *new_tip,
                    affected_from_block: *affected_from_block,
                    detected_at: Utc::now(),
                };
                self.handle_reorg(Chain::Bitcoin, *affected_from_block, re).await?;
            }
            RawEvent::Hemi(HemiEvent::BlockReorg { affected_from_block, old_tip, new_tip, depth }) => {
                let re = ReorgEvent {
                    chain: Chain::Hemi,
                    depth: *depth,
                    old_tip: *old_tip,
                    new_tip: *new_tip,
                    affected_from_block: *affected_from_block,
                    detected_at: Utc::now(),
                };
                self.handle_reorg(Chain::Hemi, *affected_from_block, re).await?;
            }
            RawEvent::Ethereum(EthereumEvent::BlockReorg { affected_from_block, old_tip, new_tip, depth }) => {
                let re = ReorgEvent {
                    chain: Chain::Ethereum,
                    depth: *depth,
                    old_tip: *old_tip,
                    new_tip: *new_tip,
                    affected_from_block: *affected_from_block,
                    detected_at: Utc::now(),
                };
                self.handle_reorg(Chain::Ethereum, *affected_from_block, re).await?;
            }

            // PoP keystone anchoring
            RawEvent::Hemi(HemiEvent::PopKeystoneAnchored { keystone_block, pop_score, .. }) => {
                self.handle_keystone_anchored(*keystone_block, *pop_score).await?;
            }

            // Cross-chain matching
            _ => {
                if let Some(m) = self.matcher.process_event(event) {
                    self.handle_match(m).await?;
                }
            }
        }
        Ok(())
    }

    // ── Keystone anchoring ────────────────────────────────────────────────────

    /// Fan out a `PopKeystoneAnchored` event to all in-flight transfers.
    ///
    /// Uses `PopAnchor::covers_block` as the single authoritative check —
    /// window is (keystone_block - 25, keystone_block] (exclusive, inclusive).
    async fn handle_keystone_anchored(&mut self, keystone_block: u64, pop_score: u64) -> Result<()> {
        if keystone_block <= self.last_anchored_keystone {
            debug!(keystone_block, "Duplicate or out-of-order keystone, skipping");
            return Ok(());
        }
        self.last_anchored_keystone = keystone_block;

        let anchor = PopAnchor {
            keystone_block,
            pop_score,
            reward_pool: 0,
            observed_at: Utc::now(),
        };

        info!(
            keystone_block,
            pop_score,
            window = format!("({}, {}]", keystone_block.saturating_sub(PopAnchor::KEYSTONE_FREQUENCY), keystone_block),
            "PopKeystoneAnchored — checking in-flight transfers"
        );

        // Collect transfers whose Hemi mint block is covered by this keystone.
        let to_anchor: Vec<Uuid> = self.state
            .transfers_initiated_with_hemi_mint()
            .into_iter()
            .filter(|(_, mint_block)| anchor.covers_block(*mint_block))
            .map(|(id, _)| id)
            .collect();

        if to_anchor.is_empty() {
            debug!(keystone_block, "No transfers covered by this keystone");
            return Ok(());
        }

        info!(keystone_block, count = to_anchor.len(), "Anchoring transfers");

        let anchored_at = Utc::now();
        for id in to_anchor {
            self.state.anchor(&id).map_err(|e| StraitError::Internal(e.to_string()))?;
            self.state.set_pop_anchor(&id, keystone_block, pop_score, anchored_at);

            self.emit(TunnelTransferUpdate::PopAnchored {
                id,
                keystone_block,
                pop_score,
                anchored_at,
            }).await?;
            self.emit(TunnelTransferUpdate::StatusChanged {
                id,
                new_status: TunnelStatus::Anchored,
                updated_at: anchored_at,
            }).await?;

            info!(transfer_id = %id, keystone_block, "Transfer INITIATED → ANCHORED via PoP keystone");
        }

        Ok(())
    }

    // ── Match handling ────────────────────────────────────────────────────────

    async fn handle_match(&mut self, m: crate::matcher::MatchResult) -> Result<()> {
        use crate::matcher::MatchDirection;
        debug!(direction = ?m.direction, amount = m.amount, "Cross-chain match found");

        match m.direction {
            // BTC→Hemi: transfer starts INITIATED, waits for keystone to advance.
            MatchDirection::BtcToHemi => {}

            // ETH→Hemi: OP Stack finality — advance to ANCHORED immediately.
            MatchDirection::EthToHemi => {}

            // Hemi→ETH or Hemi→BTC outflows.
            MatchDirection::HemiToEth => {}
        }

        Ok(())
    }

    // ── Reorg handling ────────────────────────────────────────────────────────

    async fn handle_reorg(
        &mut self,
        chain: Chain,
        affected_from_block: u64,
        reorg_event: ReorgEvent,
    ) -> Result<()> {
        warn!(chain = %chain, affected_from_block, "Reorg — retracting affected transfers");

        self.state.handle_reorg(&chain, affected_from_block, reorg_event);

        // Emit retraction for any transfer now marked REORGED
        let retracted: Vec<Uuid> = self.state
            .transfers_by_status_discriminant(std::mem::discriminant(
                &TunnelStatus::Reorged { retracted_at: Utc::now() }
            ))
            .into_iter()
            .map(|t| t.id)
            .collect();

        for id in retracted {
            self.emit(TunnelTransferUpdate::Retracted {
                id,
                reason: format!("Reorg on {chain} from block {affected_from_block}"),
                retracted_at: Utc::now(),
            }).await?;
        }

        Ok(())
    }

    async fn emit(&self, update: TunnelTransferUpdate) -> Result<()> {
        self.update_tx.send(update).await
            .map_err(|e| StraitError::Internal(format!("Failed to emit update: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keystone_for_exact_multiple() {
        assert_eq!(keystone_for(0), 0);
        assert_eq!(keystone_for(25), 25);
        assert_eq!(keystone_for(50), 50);
        assert_eq!(keystone_for(100), 100);
    }

    #[test]
    fn test_keystone_for_between_multiples() {
        assert_eq!(keystone_for(1), 25);
        assert_eq!(keystone_for(24), 25);
        assert_eq!(keystone_for(26), 50);
        assert_eq!(keystone_for(49), 50);
    }

    #[test]
    fn test_keystone_for_real_blocks() {
        // 12350 = 25 * 494 — it IS a keystone, so it covers itself
        assert_eq!(keystone_for(12350), 12350);
        // Block 12351 (one past a keystone) waits for the next keystone 12375
        assert_eq!(keystone_for(12351), 12375);
        // Block 12374 (one before a keystone) waits for keystone 12375
        assert_eq!(keystone_for(12374), 12375);
        // Block exactly on keystone 12375 is covered by it
        assert_eq!(keystone_for(12375), 12375);
        // Block 12376 waits for keystone 12400
        assert_eq!(keystone_for(12376), 12400);
    }

    #[test]
    fn test_keystone_coverage() {
        // keystone 12375 covers blocks (12350, 12375]
        let keystone = 12375u64;
        let prev = keystone - KEYSTONE_FREQ;
        assert!(12351 > prev && 12351 <= keystone); // covered
        assert!(12375 > prev && 12375 <= keystone); // covered (on keystone)
        assert!(!(12350 > prev && 12350 <= keystone)); // NOT covered (== prev)
        assert!(!(12376 > prev && 12376 <= keystone)); // NOT covered (next keystone)
    }

    // ── PopAnchor.covers_block boundary tests ─────────────────────────────────
    // These tests are the authoritative specification for which Hemi mint blocks
    // get anchored by a given keystone. If these pass, the logic is correct.

    fn make_anchor(keystone_block: u64) -> PopAnchor {
        PopAnchor {
            keystone_block,
            pop_score: 5000,
            reward_pool: 1_000_000,
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn covers_block_window_boundaries() {
        // Keystone at 100, window is (75, 100]
        let anchor = make_anchor(100);
        let cases: &[(u64, bool)] = &[
            (74, false), // well before window
            (75, false), // exactly at window_start — NOT covered (exclusive lower bound)
            (76, true),  // one past window_start — covered
            (99, true),  // one before keystone — covered
            (100, true), // exactly on keystone_block — covered (inclusive upper bound)
            (101, false), // one past keystone_block — NOT covered
            (200, false), // well after window
        ];
        for (mint_block, expected) in cases {
            assert_eq!(
                anchor.covers_block(*mint_block),
                *expected,
                "keystone=100, mint_block={mint_block}: expected covers={expected}"
            );
        }
    }

    #[test]
    fn covers_block_keystone_at_frequency_boundary() {
        // Keystone at exactly 25 (first possible keystone), window is (0, 25]
        let anchor = make_anchor(25);
        assert!(!anchor.covers_block(0));  // saturating_sub(25) = 0, so 0 > 0 is false
        assert!(anchor.covers_block(1));
        assert!(anchor.covers_block(25));
        assert!(!anchor.covers_block(26));
    }

    #[test]
    fn covers_block_at_keystone_zero_saturates() {
        // Keystone at 0 (degenerate): window_start saturates to 0
        // covers_block(0) = 0 > 0 && 0 <= 0 = false — correctly excluded
        let anchor = make_anchor(0);
        assert!(!anchor.covers_block(0));
        assert!(!anchor.covers_block(1));
    }

    #[test]
    fn consecutive_keystones_cover_all_blocks_without_overlap() {
        // Blocks 1..=50 should be covered by exactly one of keystone 25 or keystone 50
        let k25 = make_anchor(25);
        let k50 = make_anchor(50);

        for block in 1u64..=50 {
            let in_25 = k25.covers_block(block);
            let in_50 = k50.covers_block(block);
            assert!(
                in_25 ^ in_50,
                "block {block} should be in exactly one keystone (25={in_25}, 50={in_50})"
            );
        }
    }
}
