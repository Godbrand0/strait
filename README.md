# Strait

**Real-time, reorg-safe tunnel indexer for Hemi Network.**

Strait tracks assets moving through Hemi's trust-minimized bridges between Bitcoin, Hemi, and Ethereum. It ingests all three chains, joins related events into unified `TunnelTransfer` records with complete lifecycles, and serves that data via GraphQL and webhooks.

---

## What It Does

A complete tunnel flow touches at least three blockchains — each with different finality semantics and no shared identifier in raw form. Strait's job is to:

1. **Ingest** raw events from Bitcoin, Hemi, and Ethereum independently and concurrently
2. **Join** related cross-chain events into a single `TunnelTransfer` record using a stateful engine
3. **Track lifecycle** — `INITIATED → ANCHORED → FINALIZED` — with explicit handling for reorgs and failures
4. **Serve** the unified dataset via GraphQL queries, real-time webhooks, and a complete audit log

Supported tunnel routes:

| Route | Direction | Description |
|---|---|---|
| `BTC_TO_HEMI` | In | Bitcoin deposit → Hemi `DepositConfirmed` → hBTC minted |
| `HEMI_TO_BTC` | Out | `WithdrawalInitiated` on Hemi → BTC payout by vault operator |
| `ETH_TO_HEMI` | In | Ethereum `ETHBridgeInitiated` → Hemi `ETHBridgeFinalized` |
| `HEMI_TO_ETH` | Out | Hemi `ETHBridgeInitiated` → Ethereum release |

---

## Contract Addresses

All addresses confirmed from the Hemi explorer and the [`hemilabs/bitcoin-tunnel-contracts`](https://github.com/hemilabs/bitcoin-tunnel-contracts) repository.

### BTC Tunnel — `BitcoinTunnelManager`

| Network | Address |
|---|---|
| Hemi Mainnet | `0xEAcA824F46c000fB89403846Bb57e6b913321081` |
| Hemi Sepolia (testnet) | `0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e` |

### ETH/ERC-20 Tunnel — `L2StandardBridge` (OP Stack)

| Network | Address |
|---|---|
| Hemi Mainnet + Hemi Sepolia | `0x4200000000000000000000000000000000000010` |
| Ethereum Mainnet (L1) | `0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e` |
| Ethereum Sepolia (L1) | `0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925` |

### BitcoinKit Precompile

| Network | Address | Version |
|---|---|---|
| Hemi Mainnet | `0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12` | v1 |
| Hemi Sepolia | `0xeC9fa5daC1118963933e1A675a4EEA0009b7f215` | v0 |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  GraphQL API (/graphql)  │  Webhooks  │  GraphiQL Playground│
└─────────────────────────────────────────────────────────────┘
                             │
┌─────────────────────────────────────────────────────────────┐
│  Postgres (hot path)  ←  strait-store                       │
└─────────────────────────────────────────────────────────────┘
                             │
┌─────────────────────────────────────────────────────────────┐
│  Join Engine  (strait-join)                                 │
│  State machine per transfer. Reorg-aware.                   │
└─────────────────────────────────────────────────────────────┘
           │                  │                  │
┌──────────────────┐  ┌──────────────┐  ┌───────────────────┐
│ Bitcoin Ingester │  │  Hemi EVM    │  │  Ethereum EVM     │
│ + CustodyWatcher │  │  Ingester    │  │  Ingester         │
│  (strait-btc)    │  │ (strait-evm) │  │  (strait-evm)     │
└──────────────────┘  └──────────────┘  └───────────────────┘
         │                    │
    BitcoinKit v1       BitcoinTunnelManager
    precompile          + L2StandardBridge
  (reads BTC state)    (emits tunnel events)
```

### Workspace Crates

| Crate | Role |
|---|---|
| `strait-core` | Domain types, events, config, timing constants, error types |
| `strait-bitcoin` | Bitcoin ingester — CustodyWatcher via BitcoinKit, OP_RETURN parsing, reorg detection |
| `strait-evm` | EVM ingester — handles both `BitcoinTunnelManager` (BTC) and `L2StandardBridge` (ETH/ERC-20) events |
| `strait-join` | Join engine — per-route state machine, cross-chain event matching, reorg retraction |
| `strait-store` | Database layer — SQLx/Postgres CRUD for transfers, events, proofs, checkpoints |
| `strait-api` | GraphQL server (async-graphql + axum) and HMAC-signed webhook dispatcher |
| `strait-node` | Binary entrypoint — wires all crates together, manages task lifecycle |

---

## BTC Tunnel Architecture

The BTC tunnel is not a single contract. It is a hub-and-spoke system:

- **`BitcoinTunnelManager`** — central contract. Emits all indexable events. Manages vault registry, mints/burns hBTC.
- **`SimpleBitcoinVault`** (multiple) — per-operator vaults, each with its own Bitcoin custody address. Operators compete on fees and availability.
- **`BTCToken` (hBTC)** — ERC-20 with 8 decimals (satoshi precision), deployed by `BitcoinTunnelManager`.

### BTC→Hemi deposit flow

```
1. User sends BTC to vault custody address on Bitcoin.
   Transaction must include an OP_RETURN output encoding their Hemi address.

2. Anyone calls confirmDeposit(vaultIndex, txid, outputIndex, extraInfo)
   on BitcoinTunnelManager.

3. BitcoinTunnelManager emits:
   DepositConfirmed(vault, recipient, depositTxId, depositSats, netSatsAfterFee)
   ← depositTxId is the Bitcoin txid — Strait's primary cross-chain join key.

4. hBTC is minted to recipient on Hemi.
```

### Hemi→BTC withdrawal flow

```
1. User calls initiateWithdrawal(vaultIndex, btcAddress, amount)
   on BitcoinTunnelManager. hBTC is burned immediately.

2. BitcoinTunnelManager emits:
   WithdrawalInitiated(vault, withdrawer, btcAddress[hashed], withdrawalSats, netSatsAfterFee, uuid)
   ← uuid (vaultIndex << 32 | vaultSpecificUUID) is the cross-chain join key.
   ← btcAddress is indexed (hashed in topic) — original string NOT recoverable from the log.

3. Vault operator sends BTC to the user's Bitcoin address.
   Payout transaction includes an OP_RETURN encoding the uuid.

4. Strait correlates the Hemi WithdrawalInitiated with the Bitcoin payout
   by matching the uuid from the Bitcoin OP_RETURN.
```

### OP_RETURN encoding (confirmed)

Two formats, parsed from `output.script` (not `opReturnData`):

| Format | Script length | Layout |
|---|---|---|
| Raw bytes | 22 bytes | `0x6a` `0x14` + 20 raw address bytes |
| ASCII hex | 42 bytes | `0x6a` `0x28` + 40 ASCII hex characters of the address |

The OP_RETURN output must be within the first 8 outputs of the transaction.

---

## PoP Anchoring — `PoPPayoutsV2`

Hemi anchors its blocks to Bitcoin through Proof-of-Publication (PoP) miners. Strait watches `PoPPayoutsV2` to advance BTC→Hemi transfers from `INITIATED` to `ANCHORED`.

### Contract

| Network | Address |
|---|---|
| Hemi Sepolia (testnet) | `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB` |
| Hemi Mainnet | Confirm from explorer (FIXME) |

Source: [`hemilabs/pop-payouts`](https://github.com/hemilabs/pop-payouts)

### How it works

Every **25 Hemi blocks** (~5 minutes) is a **keystone**. When PoP miners publish a keystone commitment to Bitcoin, the sequencer calls `mintPoPRewards()` on `PoPPayoutsV2`, which emits:

```solidity
event PayoutRoundExecuted(uint64 indexed blockRewarded, uint256 rewardPool, uint256 popScore);
```

`blockRewarded` is always a multiple of 25. All Hemi blocks in `(blockRewarded - 25, blockRewarded]` are now PoP-anchored on Bitcoin.

### Verification logic

A transfer whose Hemi mint landed at block `N` is anchored when:

```
keystone_for(N) = ceil(N / 25) * 25
anchored when: PayoutRoundExecuted(blockRewarded >= keystone_for(N))
```

**Example:**

```
Transfer DepositConfirmed at Hemi block 12351
  keystone_for(12351) = 12375

PayoutRoundExecuted(blockRewarded=12375) fires
  → covers blocks (12350, 12375]
  → block 12351 is included ✓
  → transfer advances to ANCHORED
```

### Checking anchoring status directly

```bash
# Query lastBlockRewarded — any block <= this value is PoP-anchored
cast call 0x4a3b61C586DB4CD219E85aC0697b66916c7457AB \
  "lastBlockRewarded()(uint64)" \
  --rpc-url https://testnet.rpc.hemi.network/rpc

# Example: returns 125000
# Transfer minted at block 124990 → keystone 125000
# 125000 <= 125000 → anchored ✓
# Transfer minted at block 125001 → keystone 125025
# 125025 > 125000 → not yet anchored
```

### Note on `popScore`

A `popScore` of `0` means no miners published that keystone to Bitcoin, but the sequencer still processed the round. **A zero score does not mean the keystone is unanchored** — it means there were no publications scored, but the round was still executed and the block range is still considered PoP-finalized for Strait's purposes.

---

## Domain Model

The central object is `TunnelTransfer`:

```
TunnelTransfer
  id              UUID — globally unique cross-chain identifier
  asset           BTC | ETH | ERC20
  direction       IN (to Hemi) | OUT (from Hemi)
  route           BTC_TO_HEMI | HEMI_TO_BTC | ETH_TO_HEMI | HEMI_TO_ETH
  amount          BigDecimal (satoshis for BTC, wei for ETH, token units for ERC20)
  sender          source-chain address
  recipient       destination-chain address
  status          INITIATED | ANCHORED | FINALIZED | FAILED | REORGED
  initiated_at    timestamp
  finalized_at    timestamp (null until finalized)
  source_tx       ChainTransaction (chain, hash, block, confirmations, timestamp)
  destination_tx  ChainTransaction (null until finalized)
  pop_proofs      [] keystone anchoring records (BTC routes) — one per PayoutRoundExecuted
  reorg_events    [] full audit log of reorgs that touched this transfer
```

### Transfer lifecycle by route

**BTC→Hemi:**
```
Bitcoin UTXO detected at custody address
  → INITIATED   (Strait sees the Bitcoin deposit via BitcoinKit)

DepositConfirmed emitted on Hemi (depositTxId is the join key)
  — transfer now has a Hemi destination block N

PoPPayoutsV2.PayoutRoundExecuted(blockRewarded=K) fires, where K = ceil(N/25)*25
  → ANCHORED    (keystone K covers block N — PoP-anchored on Bitcoin)
  — typically ~5-30 min after DepositConfirmed (next keystone boundary)

Bitcoin reorg covers the deposit block → REORGED (record preserved)
```

**Hemi→BTC:**
```
WithdrawalInitiated emitted on Hemi (uuid assigned)
  → INITIATED

Bitcoin payout tx detected (OP_RETURN uuid matches)
  → ANCHORED

Bitcoin payout reaches confirmation depth
  → FINALIZED   (up to ~12 hours for operator processing)
```

**ETH→Hemi:**
```
ETHBridgeInitiated / ERC20BridgeInitiated on Ethereum
  → INITIATED

ETHBridgeFinalized / ERC20BridgeFinalized on Hemi
  → FINALIZED   (~2 minutes typical)
```

**Hemi→ETH:**
```
ETHBridgeInitiated on Hemi
  → INITIATED

Release confirmed on Ethereum
  → FINALIZED   (~40 min + up to 24h proof submission window)
```

---

## API

### GraphQL

Served at `http://localhost:8080/graphql`. GraphiQL playground at `http://localhost:8080/` in development.

**Query by Strait transfer ID:**

```graphql
query {
  tunnelTransfer(id: "550e8400-e29b-41d4-a716-446655440000") {
    id
    route
    asset
    amount
    status
    initiatedAt
    finalizedAt
    sourceTx { chain hash blockNumber confirmations }
    destinationTx { chain hash blockNumber }
    reorgEvents { chain depth affectedFromBlock detectedAt }
  }
}
```

**Query by Bitcoin deposit txid (BTC→Hemi):**

The `depositTxId` from the `DepositConfirmed` event is the join key for BTC→Hemi transfers. Query by it directly:

```graphql
query {
  tunnelTransfers(
    filter: {
      route: BTC_TO_HEMI
      sourceTxHash: "a3f7c2d1e8b4f6a9c0d2e5f8b1a4c7d0e3f6a9b2c5d8e1f4a7b0c3d6e9f2a5b8"
    }
    first: 1
  ) {
    edges {
      node {
        id
        amount
        status
        sourceTx { hash blockNumber }   # Bitcoin deposit tx
        destinationTx { hash }          # Hemi DepositConfirmed tx
      }
    }
  }
}
```

**Query by withdrawal UUID (Hemi→BTC):**

The `uuid` from `WithdrawalInitiated` is the cross-chain join key for Hemi→BTC withdrawals. It encodes `(vaultIndex << 32 | vaultSpecificUUID)`:

```graphql
query {
  tunnelTransfers(
    filter: {
      route: HEMI_TO_BTC
      withdrawalUuid: "4294967297"   # decimal string — vaultIndex=1, vaultUUID=1
    }
    first: 1
  ) {
    edges {
      node {
        id
        amount
        status
        sourceTx { hash blockNumber }   # Hemi WithdrawalInitiated tx
        destinationTx { hash }          # Bitcoin payout tx (null until operator pays)
        initiatedAt
        finalizedAt
      }
    }
  }
}
```

You can also decompose a uuid yourself:

```bash
# uuid = 8589934594
vault_index=$((8589934594 >> 32))       # = 2
vault_uuid=$((8589934594 & 0xFFFFFFFF)) # = 2
```

**List active withdrawals for a wallet (monitoring vault operator):**

```graphql
query PendingWithdrawals {
  tunnelTransfers(
    filter: {
      route: HEMI_TO_BTC
      status: INITIATED            # operator has not yet paid
      sender: "0xYourWalletAddress"
    }
    first: 50
  ) {
    edges {
      node {
        id
        amount
        initiatedAt
        sourceTx { hash }
      }
    }
  }
}
```

**Filter by vault address (operator view):**

```graphql
query VaultDeposits {
  tunnelTransfers(
    filter: {
      route: BTC_TO_HEMI
      vaultAddress: "0xVaultContractAddress"
      status: FINALIZED
    }
    first: 100
  ) {
    edges {
      node { id amount initiatedAt finalizedAt }
    }
  }
}
```

**Aggregate stats:**

```graphql
query {
  tunnelStats(window: LAST_7D) {
    totalVolumeUsd
    transferCount
    averageFinalitySecs
    activeTransfers
  }
}
```

**Cursor-paginated list:**

```graphql
query {
  tunnelTransfers(
    filter: {
      route: BTC_TO_HEMI
      status: FINALIZED
      amountGte: "1000000"         # 0.01 BTC in satoshis
      initiatedAfter: "2025-03-12T00:00:00Z"
    }
    first: 20
    after: "cursor-from-previous-page"
  ) {
    edges {
      node { id amount status initiatedAt }
      cursor
    }
    pageInfo { hasNextPage endCursor }
  }
}
```

### Webhooks

Register a webhook to receive real-time push notifications:

```bash
curl -X POST http://localhost:8080/webhooks \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://your-endpoint.example.com/hooks/strait",
    "secret": "your-hmac-signing-secret",
    "filter": {
      "routes": ["BTC_TO_HEMI"],
      "assets": ["BTC"],
      "statusTransitions": ["FINALIZED"],
      "minAmount": "100000000"
    }
  }'
```

Every delivery is signed with `HMAC-SHA256(secret, body)` in the `X-Strait-Signature` header. Deliveries retry up to 5 times with exponential backoff.

**Watch for failed withdrawals (operator not paying):**

```bash
curl -X POST http://localhost:8080/webhooks \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://your-bot.example.com/hooks/strait",
    "secret": "secret",
    "filter": {
      "routes": ["HEMI_TO_BTC"],
      "statusTransitions": ["FAILED"]
    }
  }'
```

---

## Setup

### Prerequisites

- **Rust** 1.75+ (2021 edition)
- **PostgreSQL** 14+
- **Bitcoin full node** with RPC enabled (or Bitcoin testnet node)
- **Hemi RPC endpoint** — testnet: `https://testnet.rpc.hemi.network/rpc`
- **Ethereum RPC endpoint** — e.g. Alchemy or Infura (Sepolia for testnet)
- [`sqlx-cli`](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli)

### Quick start (testnet)

```bash
git clone https://github.com/strait-data/strait
cd strait && cp .env.example .env
```

Edit `.env` — all testnet addresses are pre-filled:

```bash
# Hemi testnet
HEMI_RPC_URL=https://testnet.rpc.hemi.network/rpc
HEMI_CHAIN_ID=743111
HEMI_TUNNEL_CONTRACT=0x4200000000000000000000000000000000000010
HEMI_BTC_TUNNEL_CONTRACT=0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e
HEMI_BITCOIN_KIT_CONTRACT=0xeC9fa5daC1118963933e1A675a4EEA0009b7f215
HEMI_START_BLOCK=0
HEMI_CONFIRMATION_DEPTH=3

# Ethereum Sepolia
ETH_RPC_URL=https://eth-sepolia.g.alchemy.com/v2/YOUR_KEY
ETH_CHAIN_ID=11155111
ETH_TUNNEL_CONTRACT=0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925
ETH_CONFIRMATION_DEPTH=12

# Bitcoin (optional — BitcoinKit covers most use cases)
BITCOIN_RPC_URL=http://localhost:8332
BITCOIN_RPC_USER=user
BITCOIN_RPC_PASSWORD=password
BITCOIN_CONFIRMATION_DEPTH=6

# Database
DATABASE_URL=postgres://postgres:password@localhost:5432/strait

# API
API_HOST=0.0.0.0
API_PORT=8080
```

```bash
# Start Postgres
docker run -d --name strait-pg \
  -e POSTGRES_PASSWORD=password \
  -e POSTGRES_DB=strait \
  -p 5432:5432 postgres:16

# Run migrations and start
sqlx migrate run
cargo run --release -p strait-node
```

Shutdown cleanly with `Ctrl+C` — all tasks drain before exiting.

### Mainnet addresses

```bash
HEMI_RPC_URL=https://rpc.hemi.network/rpc
HEMI_CHAIN_ID=43111
HEMI_TUNNEL_CONTRACT=0x4200000000000000000000000000000000000010
HEMI_BTC_TUNNEL_CONTRACT=0xEAcA824F46c000fB89403846Bb57e6b913321081
HEMI_BITCOIN_KIT_CONTRACT=0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY
ETH_CHAIN_ID=1
ETH_TUNNEL_CONTRACT=0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e
```

---

## Configuration Reference

| Variable | Default | Description |
|---|---|---|
| `BITCOIN_RPC_URL` | — | Bitcoin node RPC URL |
| `BITCOIN_RPC_USER` | — | RPC username |
| `BITCOIN_RPC_PASSWORD` | — | RPC password |
| `BITCOIN_TUNNEL_ADDRESSES` | — | Comma-separated vault custody addresses to watch |
| `BITCOIN_CONFIRMATION_DEPTH` | `6` | Blocks before a Bitcoin event is stable (~1 hour) |
| `HEMI_RPC_URL` | — | Hemi RPC endpoint |
| `HEMI_CHAIN_ID` | `43111` | Chain ID (743111 for testnet) |
| `HEMI_TUNNEL_CONTRACT` | `0x4200...0010` | L2StandardBridge — ETH/ERC-20 routes |
| `HEMI_BTC_TUNNEL_CONTRACT` | — | BitcoinTunnelManager — BTC routes |
| `HEMI_BITCOIN_KIT_CONTRACT` | — | BitcoinKit precompile |
| `HEMI_START_BLOCK` | `0` | Block to begin indexing from |
| `HEMI_CONFIRMATION_DEPTH` | `3` | Blocks before a Hemi event is stable |
| `ETH_RPC_URL` | — | Ethereum RPC endpoint |
| `ETH_CHAIN_ID` | `1` | Chain ID (11155111 for Sepolia) |
| `ETH_TUNNEL_CONTRACT` | — | L1StandardBridgeProxy on Ethereum |
| `ETH_START_BLOCK` | `0` | Block to begin indexing from |
| `ETH_CONFIRMATION_DEPTH` | `12` | Blocks before an Ethereum event is stable |
| `DATABASE_URL` | — | PostgreSQL connection string |
| `API_HOST` | `0.0.0.0` | API bind address |
| `API_PORT` | `8080` | API bind port |

---

## Reorg Safety

Reorgs are treated as normal control flow, not edge cases:

- Each ingester maintains a rolling window of recent block hashes beyond its confirmation depth
- On each new block, the parent hash is verified against the stored hash for the prior block
- A mismatch triggers a backward walk to find the fork point, emitting a `BlockReorg` event
- The join engine retracts any in-flight transfers whose source transactions were reorged out
- Retractions are stored with a `reorg_at` timestamp — records are never hard-deleted

| Chain | Default confirmation depth | Approximate window |
|---|---|---|
| Bitcoin | 6 | ~1 hour |
| Hemi | 3 | ~30 seconds |
| Ethereum | 12 | ~3 minutes |

---

## Development

### Run tests

```bash
# Unit tests (no external dependencies)
cargo test -p strait-core
cargo test -p strait-join
cargo test -p strait-bitcoin   # includes OP_RETURN parsing tests

# Integration tests (require a running Postgres)
DATABASE_URL=postgres://... cargo test -p strait-store
```

### Structured logging

```bash
RUST_LOG=strait=debug,sqlx=warn cargo run -p strait-node
```

Key log lines to watch when verifying the indexer is working:

```
INFO strait_evm::ingester: DepositConfirmed — BTC deposited and hBTC minted on Hemi
INFO strait_evm::ingester: ETHBridgeFinalized (deposit on Hemi)
INFO strait_evm::ingester: New BTC tunnel vault created — add to watched addresses
INFO strait_evm::ingester: PayoutRoundExecuted — Hemi blocks anchored on Bitcoin keystone_block=12375 pop_score=342000
INFO strait_join::engine: Anchoring transfers count=3 keystone_block=12375
INFO strait_join::engine: Transfer INITIATED → ANCHORED transfer_id=550e8400-...
```

### Database migrations

```bash
sqlx migrate run       # apply pending
sqlx migrate revert    # revert last
sqlx migrate add <name>
```

---

## Open Items

| Item | Status |
|---|---|
| ETH/ERC-20 tunnel ABI | Confirmed — OP Stack `L2StandardBridge` events |
| BTC tunnel contract address + ABI | Confirmed — `BitcoinTunnelManager` from [`hemilabs/bitcoin-tunnel-contracts`](https://github.com/hemilabs/bitcoin-tunnel-contracts) |
| OP_RETURN encoding | Confirmed — 22-byte (raw) or 42-byte (ASCII hex) from `SimpleBitcoinVaultUTXOLogicHelper.sol` |
| PoP proof contract | Confirmed — `PoPPayoutsV2` via keystone anchoring from [`hemilabs/pop-payouts`](https://github.com/hemilabs/pop-payouts). Testnet: `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB`. Mainnet address pending. |
| `PoPPayoutsV2` mainnet address | **Still open** — confirm from Hemi mainnet explorer |
| Reorg frequency in production | **Still open** — ask at Hemi office hour |

---

## License

MIT — see [LICENSE](LICENSE) for details.

---

## Related Documents

- [`docs/contract-addresses.md`](docs/contract-addresses.md) — all confirmed contract addresses and hBK interface reference
- [`files (3)/01-strait-tunnel-indexer.md`](files%20(3)/01-strait-tunnel-indexer.md) — product specification and rationale
- [`files (3)/02-strait-platform.md`](files%20(3)/02-strait-platform.md) — long-term platform vision
- [`files (3)/03-strait-wedge-to-platform-strategy.md`](files%20(3)/03-strait-wedge-to-platform-strategy.md) — sequencing strategy
