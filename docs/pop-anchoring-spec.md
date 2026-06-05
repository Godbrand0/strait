# Strait — Q3 Integration: PoP Anchoring via PoPPayoutsV2

## Context

Q3 (PoP proof contract) is resolved. The contract is `PoPPayoutsV2`, not a
per-transfer event emitter. This prompt updates the domain model, ingester,
join engine, and store to reflect how PoP anchoring actually works on Hemi.

Read this fully before touching any file.

## Your First Action

```bash
cargo check --workspace 2>&1
cargo test --workspace 2>&1
```

Zero errors, zero warnings. Fix before proceeding.

---

## How PoP Anchoring Actually Works

The original design assumed per-transfer PoP events. The reality is different.

`PoPPayoutsV2` emits `PayoutRoundExecuted(uint64 indexed blockRewarded, ...)` once
every 25 Hemi blocks (a "keystone"). This single event means: "keystone block K
is now anchored to Bitcoin." Any transfer whose Hemi mint block falls in the range
`(K - 24, K]` is anchored by that event.

A transfer is `PopAnchored` when:
1. Its Hemi mint is confirmed (`HemiConfirmed`)
2. A `PayoutRoundExecuted` fires with `blockRewarded >= transfer.hemi_mint_block`

The anchoring window is approximately 90 minutes after the Hemi mint (~9 Bitcoin
blocks for PoP publication to be accepted). This two-stage finality model —
fast Hemi confirmation in seconds, Bitcoin-anchored finality in ~90 minutes —
is now correctly represented.

**What we cannot get from events**: the specific Bitcoin txid that carried the
PoP publication. That data comes in as calldata to `mintPoPRewards()`, not as an
indexed event. We skip this in v1. The product story is "anchored to Bitcoin" not
"here is the specific Bitcoin txid."

**Key constants from PoPPayoutsV2:**

```
KEYSTONE_FREQUENCY     = 25 Hemi blocks     (~5 min per keystone)
BITCOIN_FINALITY_DELAY = 9 BTC blocks       (~90 min anchoring window)
```

---

## Step 1 — Update the Domain Model in `strait-core`

### Replace `PopProof` with `PopAnchor` in `crates/strait-core/src/types.rs`

Remove `PopProof` entirely. Add `PopAnchor`:

```rust
/// A confirmed PoP anchoring event from PoPPayoutsV2.
/// One anchor can cover many transfers — any transfer whose Hemi mint block
/// falls in the 25-block keystone window is anchored by the same event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopAnchor {
    /// The Hemi keystone block that was anchored to Bitcoin.
    pub keystone_block: u64,

    /// The aggregate PoP score for this keystone.
    /// Higher = more miners published, more quickly.
    /// Stored now, exposed in API later if needed.
    pub pop_score: u64,

    /// The reward pool for this keystone round (in wei).
    pub reward_pool: u64,

    /// When Strait observed this anchor event.
    pub observed_at: DateTime<Utc>,
}

impl PopAnchor {
    /// Returns true if the given Hemi block falls within this keystone's window.
    /// Keystone window: (keystone_block - KEYSTONE_FREQUENCY, keystone_block]
    pub const KEYSTONE_FREQUENCY: u64 = 25;

    pub fn covers_block(&self, hemi_block: u64) -> bool {
        let window_start = self.keystone_block.saturating_sub(Self::KEYSTONE_FREQUENCY);
        hemi_block > window_start && hemi_block <= self.keystone_block
    }
}
```

### Update `TunnelTransfer` in `crates/strait-core/src/types.rs`

Replace the `pop_proofs: Vec<PopProof>` field with:

```rust
pub struct TunnelTransfer {
    pub id:               Uuid,
    pub asset:            Asset,
    pub direction:        TunnelDirection,
    pub route:            TunnelRoute,
    pub amount:           BigDecimal,
    pub sender:           ChainAddress,
    pub recipient:        ChainAddress,
    pub status:           TunnelStatus,
    pub initiated_at:     DateTime<Utc>,
    pub finalized_at:     Option<DateTime<Utc>>,
    pub source_tx:        ChainTransaction,
    pub destination_tx:   Option<ChainTransaction>,

    // PoP anchoring — replaces Vec<PopProof>
    pub pop_anchored:      bool,
    pub pop_keystone_block: Option<u64>,
    pub pop_score:          Option<u64>,
    pub pop_anchored_at:    Option<DateTime<Utc>>,

    pub reorg_events:     Vec<ReorgEvent>,
}
```

### Update `HemiEvent` in `crates/strait-core/src/events.rs`

Replace `PopProofSubmitted` with `PopKeystoneAnchored`:

```rust
/// PoPPayoutsV2::PayoutRoundExecuted
/// Fires every 25 Hemi blocks when a keystone is anchored to Bitcoin.
/// One event anchors all transfers whose Hemi mint fell in that keystone window.
PopKeystoneAnchored {
    hemi_tx_hash:   TxHash,
    keystone_block: u64,    // blockRewarded — the exact Hemi block anchored
    reward_pool:    u64,    // rewardPool in wei
    pop_score:      u64,    // aggregate PoP quality score
    block_number:   u64,    // Hemi block the event was emitted in
    log_index:      u32,
},
```

---

## Step 2 — Add PoPPayoutsV2 ABI to `crates/strait-evm/src/contracts.rs`

Add alongside the existing contracts. Do not remove or change anything else:

```rust
// PoPPayoutsV2 — confirmed from Hemi GitHub
// Fires PayoutRoundExecuted every 25 Hemi blocks (one keystone)
// The only event useful for Strait — per-transfer PoP data is in calldata,
// not in events, so we use keystone block ranges to infer per-transfer anchoring.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    PoPPayoutsV2,
    r#"[
        {
            "type": "event",
            "name": "PayoutRoundExecuted",
            "inputs": [
                {
                    "name": "blockRewarded",
                    "type": "uint64",
                    "indexed": true
                },
                {
                    "name": "rewardPool",
                    "type": "uint256",
                    "indexed": false
                },
                {
                    "name": "popScore",
                    "type": "uint256",
                    "indexed": false
                }
            ]
        },
        {
            "type": "event",
            "name": "RoundsBackfilled",
            "inputs": [
                {
                    "name": "startBlock",
                    "type": "uint64",
                    "indexed": true
                },
                {
                    "name": "endBlock",
                    "type": "uint64",
                    "indexed": true
                },
                {
                    "name": "count",
                    "type": "uint256",
                    "indexed": false
                }
            ]
        }
    ]"#
);

pub mod addresses {
    // existing addresses unchanged ...

    // PoPPayoutsV2 — confirmed from Hemi GitHub
    pub const POP_PAYOUTS_TESTNET_A: &str = "0x4a3b61C586DB4CD219E85aC0697b66916c7457AB";
    pub const POP_PAYOUTS_TESTNET_B: &str = "0xD50B57e62F6638413B31Eb6b32cDF3ffEff914Af";
    // FIXME: Mainnet address not yet indexed — check hemi.json deployment parameters
    pub const POP_PAYOUTS_MAINNET:   &str = "0x0000000000000000000000000000000000000000";
}
```

---

## Step 3 — Update `EvmIngester` to Watch PoPPayoutsV2

The Hemi ingester now needs to watch **two** contracts simultaneously:
- `BitcoinTunnelManager` (existing)
- `PoPPayoutsV2` (new)

### Update `crates/strait-evm/src/ingester.rs`

**Constructor**: accept a `Vec<AlloyAddress>` of contracts to watch, replacing
the single `tunnel_contract` field:

```rust
pub struct EvmIngester {
    provider:          Arc<RootProvider<Http<Client>>>,
    chain:             Chain,
    config:            EvmChainConfig,
    watched_contracts: Vec<AlloyAddress>,   // replaces single tunnel_contract
    event_tx:          mpsc::Sender<RawEvent>,
    tip_tracker:       TipTracker,
    last_processed_block: u64,
}

impl EvmIngester {
    pub async fn new_hemi(
        config: EvmChainConfig,
        event_tx: mpsc::Sender<RawEvent>,
        start_from_block: u64,
        is_testnet: bool,
    ) -> Result<Self, StraitError> {
        use crate::contracts::addresses::*;

        let btc_tunnel = if is_testnet { BTC_TUNNEL_TESTNET } else { BTC_TUNNEL_MAINNET };
        let pop_a      = POP_PAYOUTS_TESTNET_A;
        let pop_b      = POP_PAYOUTS_TESTNET_B;

        let watched_contracts = if is_testnet {
            vec![
                btc_tunnel.parse().map_err(|_| StraitError::Config("Invalid BTC tunnel address".into()))?,
                pop_a.parse().map_err(|_| StraitError::Config("Invalid PoP address A".into()))?,
                pop_b.parse().map_err(|_| StraitError::Config("Invalid PoP address B".into()))?,
            ]
        } else {
            vec![
                btc_tunnel.parse().map_err(|_| StraitError::Config("Invalid BTC tunnel address".into()))?,
                // Mainnet PoP address — add when confirmed
            ]
        };

        Self::new_with_contracts(config, Chain::Hemi, event_tx, start_from_block, watched_contracts).await
    }

    pub async fn new_with_contracts(
        config: EvmChainConfig,
        chain: Chain,
        event_tx: mpsc::Sender<RawEvent>,
        start_from_block: u64,
        watched_contracts: Vec<AlloyAddress>,
    ) -> Result<Self, StraitError> {
        // ... same as existing new() but uses watched_contracts vec
    }
}
```

**`process_block_range`**: build the log filter with all watched contracts:

```rust
async fn process_block_range(&self, from: u64, to: u64) -> Result<(), StraitError> {
    let filter = Filter::new()
        .address(self.watched_contracts.clone())  // Vec<Address> — alloy accepts this
        .from_block(from)
        .to_block(to);

    let logs = self.provider.get_logs(&filter).await
        .map_err(|e| StraitError::Provider(e.to_string()))?;

    for log in logs {
        let events = self.decode_hemi_log(&log)?;
        for event in events {
            self.event_tx.send(event).await
                .map_err(|_| StraitError::ChannelClosed)?;
        }
    }

    Ok(())
}
```

**`decode_hemi_log`**: add PoP decoding after the existing BTC tunnel decoders:

```rust
fn decode_hemi_log(&self, log: &Log) -> Result<Vec<RawEvent>, StraitError> {
    use crate::contracts::{BitcoinTunnelManager, PoPPayoutsV2};

    // ... existing BitcoinTunnelManager decoders (DepositConfirmed, etc.) ...

    // PayoutRoundExecuted — keystone anchored to Bitcoin
    if let Ok(decoded) = PoPPayoutsV2::PayoutRoundExecuted::decode_log(log.as_ref(), true) {
        let keystone_block = decoded.blockRewarded;
        let reward_pool    = u64::try_from(decoded.rewardPool).unwrap_or(u64::MAX);
        let pop_score      = u64::try_from(decoded.popScore).unwrap_or(0);

        tracing::info!(
            keystone_block,
            pop_score,
            "PoP keystone anchored to Bitcoin"
        );

        return Ok(vec![RawEvent::Hemi(HemiEvent::PopKeystoneAnchored {
            hemi_tx_hash:   TxHash::from(log.transaction_hash.unwrap_or_default().0),
            keystone_block,
            reward_pool,
            pop_score,
            block_number:   log.block_number.unwrap_or_default(),
            log_index:      log.log_index.unwrap_or_default() as u32,
        })]);
    }

    // RoundsBackfilled — log at debug, no action needed for Strait
    if let Ok(decoded) = PoPPayoutsV2::RoundsBackfilled::decode_log(log.as_ref(), true) {
        tracing::debug!(
            start_block = decoded.startBlock,
            end_block   = decoded.endBlock,
            count       = %decoded.count,
            "PoP rounds backfilled — no action required"
        );
        return Ok(vec![]);
    }

    // ... existing unknown event handling ...
}
```

---

## Step 4 — Update the Join Engine

### Update `crates/strait-join/src/state.rs`

**`InFlightTransfer`**: replace `pop_proofs: Vec<PopProof>` with anchoring fields:

```rust
pub struct InFlightTransfer {
    pub id:           Uuid,
    pub join_key:     JoinKey,
    pub state:        TransferState,
    pub created_at:   DateTime<Utc>,
    pub updated_at:   DateTime<Utc>,

    // PoP anchoring — replaces pop_proofs vec
    pub pop_anchored:       bool,
    pub pop_keystone_block: Option<u64>,
    pub pop_score:          Option<u64>,
    pub pop_anchored_at:    Option<DateTime<Utc>>,

    // Hemi mint block — needed to check if a keystone covers this transfer
    pub hemi_mint_block: Option<u64>,

    pub source_tx:          Option<ChainTransaction>,
    pub destination_tx:     Option<ChainTransaction>,
    pub amount_sats_or_wei: Option<u128>,
    pub sender:             Option<ChainAddress>,
    pub recipient:          Option<ChainAddress>,
    pub route:              Option<TunnelRoute>,
    pub asset:              Option<Asset>,
    pub source_block:       Option<u64>,
    pub destination_block:  Option<u64>,
}
```

**Update `apply_hemi`**: when processing `HemiEvent::BitcoinDepositConfirmed`
or `HemiEvent::EthBridgeFinalized`, store the Hemi mint block:

```rust
// In the HemiConfirmed transition, always store the mint block
self.hemi_mint_block = Some(block_number);
```

**Update `apply_hemi` for `PopKeystoneAnchored`**:

```rust
HemiEvent::PopKeystoneAnchored {
    keystone_block,
    pop_score,
    reward_pool,
    ..
} => {
    // Check if this transfer's Hemi mint falls in this keystone's window
    let Some(mint_block) = self.hemi_mint_block else {
        // Transfer doesn't have a Hemi mint yet — keystone irrelevant
        return Ok(vec![]);
    };

    let window_start = keystone_block.saturating_sub(PopAnchor::KEYSTONE_FREQUENCY);
    let covered = mint_block > window_start && mint_block <= *keystone_block;

    if !covered {
        return Ok(vec![]);
    }

    // Already anchored — idempotent
    if self.pop_anchored {
        tracing::debug!(
            id            = %self.id,
            keystone_block,
            "Duplicate PopKeystoneAnchored — already anchored, ignoring"
        );
        return Ok(vec![]);
    }

    // Anchor this transfer
    self.pop_anchored       = true;
    self.pop_keystone_block = Some(*keystone_block);
    self.pop_score          = Some(*pop_score);
    self.pop_anchored_at    = Some(Utc::now());
    self.updated_at         = Utc::now();

    tracing::info!(
        id             = %self.id,
        keystone_block,
        pop_score,
        mint_block,
        "Transfer PoP-anchored to Bitcoin"
    );

    let mut updates = vec![TunnelTransferUpdate::PopAnchored {
        id:             self.id,
        keystone_block: *keystone_block,
        pop_score:      *pop_score,
        anchored_at:    self.pop_anchored_at.unwrap(),
    }];

    // Advance state if in HemiConfirmed
    if self.state == TransferState::HemiConfirmed {
        self.transition_to(TransferState::PopAnchored)?;
        updates.push(TunnelTransferUpdate::StatusChanged {
            id:         self.id,
            new_status: TunnelStatus::Finalized,
            updated_at: Utc::now(),
        });
    }

    Ok(updates)
}
```

### Update `TunnelTransferUpdate` in `crates/strait-join/src/engine.rs`

Replace `PopProofAdded` with `PopAnchored`:

```rust
pub enum TunnelTransferUpdate {
    Created(TunnelTransfer),

    StatusChanged {
        id:         Uuid,
        new_status: TunnelStatus,
        updated_at: DateTime<Utc>,
    },

    DestinationConfirmed {
        id:             Uuid,
        destination_tx: ChainTransaction,
        finalized_at:   DateTime<Utc>,
    },

    /// A PoP keystone anchored this transfer to Bitcoin.
    /// The transfer's Hemi mint block fell within the keystone window.
    PopAnchored {
        id:             Uuid,
        keystone_block: u64,
        pop_score:      u64,
        anchored_at:    DateTime<Utc>,
    },

    Retracted {
        id:           Uuid,
        reason:       String,
        retracted_at: DateTime<Utc>,
    },
}
```

### Update `handle_pop_event` in `crates/strait-join/src/engine.rs`

The fan-out handler for `PopKeystoneAnchored` is the most important change.
It must iterate all in-flight transfers and check coverage:

```rust
async fn handle_pop_keystone(&mut self, event: RawEvent) -> Result<(), StraitError> {
    let (keystone_block, pop_score) = match &event {
        RawEvent::Hemi(HemiEvent::PopKeystoneAnchored {
            keystone_block, pop_score, ..
        }) => (*keystone_block, *pop_score),
        _ => return Ok(()),
    };

    tracing::debug!(
        keystone_block,
        pop_score,
        in_flight = self.in_flight.len(),
        "Processing PoP keystone — checking all in-flight transfers"
    );

    let mut all_updates: Vec<TunnelTransferUpdate> = Vec::new();

    for (_, transfer) in &mut self.in_flight {
        let updates = match transfer.apply(&event) {
            Ok(u)  => u,
            Err(e) => {
                tracing::warn!(error = %e, "PoP keystone apply error");
                continue;
            }
        };
        all_updates.extend(updates);
    }

    for update in all_updates {
        self.transfer_tx.send(update).await
            .map_err(|_| StraitError::ChannelClosed)?;
    }

    Ok(())
}
```

Update `process_event` to route `PopKeystoneAnchored` to the fan-out handler:

```rust
// Replace the old PopProofSubmitted routing with:
RawEvent::Hemi(HemiEvent::PopKeystoneAnchored { .. }) => {
    return self.handle_pop_keystone(event).await;
}
```

---

## Step 5 — Update `strait-store`

### Update the migrations

Add `pop_anchoring` columns to `tunnel_transfers` in a new migration file:

**`migrations/002_pop_anchoring.sql`**:

```sql
-- Replace pop_proof_count with proper pop anchoring fields
ALTER TABLE tunnel_transfers
    DROP COLUMN IF EXISTS pop_proof_count,
    ADD COLUMN pop_anchored        BOOLEAN     NOT NULL DEFAULT FALSE,
    ADD COLUMN pop_keystone_block  BIGINT,
    ADD COLUMN pop_score           BIGINT,
    ADD COLUMN pop_anchored_at     TIMESTAMPTZ;

CREATE INDEX idx_transfers_pop_anchored ON tunnel_transfers(pop_anchored);
CREATE INDEX idx_transfers_pop_keystone ON tunnel_transfers(pop_keystone_block);

-- Drop pop_proofs table — replaced by per-transfer anchor columns
DROP TABLE IF EXISTS pop_proofs;
```

### Update `crates/strait-store/src/transfers.rs`

Replace `insert_pop_proof` with `set_pop_anchored`:

```rust
/// Mark a transfer as PoP-anchored to Bitcoin.
/// Idempotent — safe to call multiple times (first call wins).
pub async fn set_pop_anchored(
    pool: &PgPool,
    id: Uuid,
    keystone_block: u64,
    pop_score: u64,
    anchored_at: DateTime<Utc>,
) -> Result<(), StraitError> {
    sqlx::query!(
        r#"
        UPDATE tunnel_transfers
        SET
            pop_anchored       = TRUE,
            pop_keystone_block = $2,
            pop_score          = $3,
            pop_anchored_at    = $4,
            updated_at         = NOW()
        WHERE id = $1
          AND pop_anchored = FALSE   -- idempotent: only update if not yet anchored
        "#,
        id,
        keystone_block as i64,
        pop_score as i64,
        anchored_at,
    )
    .execute(pool)
    .await
    .map_err(|e| StraitError::Database(e.to_string()))?;

    Ok(())
}
```

### Update `crates/strait-store/src/consumer.rs`

Replace `PopProofAdded` with `PopAnchored`:

```rust
TunnelTransferUpdate::PopAnchored {
    id,
    keystone_block,
    pop_score,
    anchored_at,
} => {
    tracing::info!(
        id             = %id,
        keystone_block,
        pop_score,
        "Transfer PoP-anchored"
    );
    transfers::set_pop_anchored(pool, *id, *keystone_block, *pop_score, *anchored_at).await
}
```

---

## Step 6 — Update the GraphQL Schema

In `crates/strait-api/src/graphql/schema.rs`, update `TunnelTransferGql`:

```rust
#[derive(SimpleObject)]
pub struct TunnelTransferGql {
    pub id:               Uuid,
    pub asset:            String,
    pub direction:        String,
    pub route:            String,
    pub amount:           String,
    pub sender:           String,
    pub recipient:        String,
    pub status:           String,
    pub initiated_at:     DateTime<Utc>,
    pub finalized_at:     Option<DateTime<Utc>>,
    pub source_chain:     String,
    pub source_tx_hash:   String,
    pub destination_chain:    Option<String>,
    pub destination_tx_hash:  Option<String>,

    // PoP anchoring — replaces pop_proof_count
    pub pop_anchored:          bool,
    pub pop_keystone_block:    Option<i64>,
    pub pop_score:             Option<i64>,
    pub pop_anchored_at:       Option<DateTime<Utc>>,
}
```

This is now directly queryable by dApps:

```graphql
{
  tunnelTransfers(filter: { status: FINALIZED }) {
    edges {
      node {
        id
        status
        popAnchored          # true/false — is this Bitcoin-final?
        popKeystoneBlock     # which Hemi keystone anchored it
        popScore             # how strong was the PoP coverage (optional)
        popAnchoredAt        # when anchoring was confirmed
      }
    }
  }
}
```

---

## Step 7 — Update Unit Tests

In `crates/strait-join/tests/state_machine.rs`, replace all PoP-related tests:

```rust
use strait_core::events::HemiEvent;

fn make_pop_keystone(keystone_block: u64, pop_score: u64) -> RawEvent {
    RawEvent::Hemi(HemiEvent::PopKeystoneAnchored {
        hemi_tx_hash:   evm_hash(99),
        keystone_block,
        reward_pool:    1_000_000,
        pop_score,
        block_number:   keystone_block + 5,
        log_index:      0,
    })
}

// ── PoP ANCHORING TESTS ────────────────────────────────────────────

#[test]
fn keystone_covering_mint_block_anchors_transfer() {
    let txid = btc_txid(20);
    let key  = JoinKey::BitcoinTxid(txid.clone());
    let mut t = InFlightTransfer::new(key);

    t.apply(&make_btc_deposit(txid.clone(), 840_000)).unwrap();

    // Hemi mint at block 500
    t.apply(&make_hemi_mint(txid.clone(), 500)).unwrap();
    assert_eq!(t.state, TransferState::HemiConfirmed);
    assert_eq!(t.hemi_mint_block, Some(500));

    // Keystone at block 525 covers window (500, 525]
    // Block 500 is NOT in this window (500 > 500 is false)
    let updates = t.apply(&make_pop_keystone(525, 9000)).unwrap();
    assert!(updates.is_empty()); // block 500 NOT covered by (500, 525]

    // Keystone at block 500 covers window (475, 500]
    // Block 500 IS in this window (500 > 475 && 500 <= 500)
    let updates = t.apply(&make_pop_keystone(500, 9000)).unwrap();
    assert_eq!(updates.len(), 2); // PopAnchored + StatusChanged
    assert!(matches!(updates[0], TunnelTransferUpdate::PopAnchored { .. }));
    assert_eq!(t.state, TransferState::PopAnchored);
    assert!(t.pop_anchored);
    assert_eq!(t.pop_keystone_block, Some(500));
    assert_eq!(t.pop_score, Some(9000));
}

#[test]
fn keystone_not_covering_mint_block_is_no_op() {
    let txid = btc_txid(21);
    let key  = JoinKey::BitcoinTxid(txid.clone());
    let mut t = InFlightTransfer::new(key);

    t.apply(&make_btc_deposit(txid.clone(), 840_000)).unwrap();
    t.apply(&make_hemi_mint(txid.clone(), 600)).unwrap();
    assert_eq!(t.hemi_mint_block, Some(600));

    // Keystone at 500 covers (475, 500] — does NOT cover block 600
    let updates = t.apply(&make_pop_keystone(500, 5000)).unwrap();
    assert!(updates.is_empty());
    assert!(!t.pop_anchored);
    assert_eq!(t.state, TransferState::HemiConfirmed); // unchanged
}

#[test]
fn duplicate_pop_keystone_is_idempotent() {
    let txid = btc_txid(22);
    let key  = JoinKey::BitcoinTxid(txid.clone());
    let mut t = InFlightTransfer::new(key);

    t.apply(&make_btc_deposit(txid.clone(), 840_000)).unwrap();
    t.apply(&make_hemi_mint(txid.clone(), 500)).unwrap();

    // First keystone anchors
    let updates = t.apply(&make_pop_keystone(500, 8000)).unwrap();
    assert!(!updates.is_empty());
    assert!(t.pop_anchored);

    // Second keystone for same range — idempotent, no updates
    let updates = t.apply(&make_pop_keystone(500, 8000)).unwrap();
    assert!(updates.is_empty());
}

#[test]
fn pop_keystone_before_hemi_mint_is_no_op() {
    let txid = btc_txid(23);
    let key  = JoinKey::BitcoinTxid(txid.clone());
    let mut t = InFlightTransfer::new(key);

    // Only deposit — no Hemi mint yet
    t.apply(&make_btc_deposit(txid.clone(), 840_000)).unwrap();
    assert_eq!(t.hemi_mint_block, None);

    // Keystone fires — no hemi_mint_block to check against
    let updates = t.apply(&make_pop_keystone(500, 5000)).unwrap();
    assert!(updates.is_empty());
    assert!(!t.pop_anchored);
}

#[test]
fn keystone_window_boundary_conditions() {
    // Keystone at 100, window is (75, 100]
    // Test all boundary values
    let cases: &[(u64, bool)] = &[
        (75, false),  // exactly at window_start — NOT covered (exclusive lower bound)
        (76, true),   // one past window_start — covered
        (100, true),  // exactly at keystone_block — covered (inclusive upper bound)
        (101, false), // one past keystone_block — NOT covered
        (50, false),  // well outside window
    ];

    for (mint_block, expected_covered) in cases {
        let anchor = strait_core::types::PopAnchor {
            keystone_block: 100,
            pop_score:      5000,
            reward_pool:    0,
            observed_at:    chrono::Utc::now(),
        };
        assert_eq!(
            anchor.covers_block(*mint_block),
            *expected_covered,
            "mint_block={mint_block} expected covered={expected_covered}"
        );
    }
}
```

---

## Verification Checklist

```bash
# All migrations apply cleanly
DATABASE_URL=postgres://postgres:password@localhost:5432/strait \
  cargo sqlx migrate run 2>&1

# Full build — zero errors, zero warnings
cargo check --workspace 2>&1
cargo clippy --workspace -- -D warnings 2>&1

# All tests pass — focus on join engine
cargo test -p strait-join 2>&1
cargo test -p strait-core 2>&1
cargo test --workspace 2>&1

# Formatting
cargo fmt --check 2>&1
```

Pay special attention to the boundary condition test for `covers_block`. The
exclusive lower bound (`hemi_block > window_start`) vs inclusive upper bound
(`hemi_block <= keystone_block`) is the subtlety that determines which transfers
get anchored by which keystone. If that test passes, the logic is correct.

---

## What Q3 Actually Gives Us

Once this is deployed against testnet, `PayoutRoundExecuted` fires every ~5 minutes.
Every in-flight transfer whose Hemi mint falls in that 25-block window automatically
advances to `PopAnchored` / `Finalized`. No per-transfer PoP tracking needed.

The two-stage finality is now correctly modelled and queryable:

```
BTC deposit observed        → status: INITIATED     (~immediate)
Hemi mint confirmed         → status: ANCHORED      (~minutes)
PoP keystone fires          → status: FINALIZED     (~90 minutes)
popAnchored: true in API    → Bitcoin-final, safe to settle against
```

This is the data no other indexer produces for Hemi. It is the core value
proposition of Strait in a single queryable field.
