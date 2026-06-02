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
| `BTC_TO_HEMI` | In | Bitcoin deposit → Hemi mint of hBTC |
| `HEMI_TO_BTC` | Out | Hemi burn → Bitcoin withdrawal |
| `ETH_TO_HEMI` | In | Ethereum lock → Hemi mint |
| `HEMI_TO_ETH` | Out | Hemi burn → Ethereum release |

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  GraphQL API (/graphql)  │  Webhooks  │  Playground  │
└──────────────────────────────────────────────────────┘
                            │
┌──────────────────────────────────────────────────────┐
│  Postgres (hot path)  ←  strait-store                │
└──────────────────────────────────────────────────────┘
                            │
┌──────────────────────────────────────────────────────┐
│  Join Engine  (strait-join)                          │
│  State machine per transfer. Reorg-aware.            │
└──────────────────────────────────────────────────────┘
           │                │                │
┌──────────────┐  ┌──────────────┐  ┌──────────────────┐
│  Bitcoin     │  │  Hemi EVM    │  │  Ethereum EVM    │
│  Ingester    │  │  Ingester    │  │  Ingester        │
│(strait-btc)  │  │(strait-evm)  │  │  (strait-evm)   │
└──────────────┘  └──────────────┘  └──────────────────┘
```

### Workspace Crates

| Crate | Role |
|---|---|
| `strait-core` | Domain types, events, config, error types |
| `strait-bitcoin` | Bitcoin chain ingester — polls full node, watches tunnel addresses and OP_RETURN data |
| `strait-evm` | Generic EVM ingester — used for both Hemi and Ethereum |
| `strait-join` | Join engine — state machine that correlates cross-chain events into `TunnelTransfer` records |
| `strait-store` | Database layer — SQLx/Postgres CRUD for transfers, events, proofs, checkpoints |
| `strait-api` | GraphQL server (async-graphql + axum) and webhook dispatcher |
| `strait-node` | Binary entrypoint — wires all crates together, manages task lifecycle |

---

## Tech Stack

| Concern | Library |
|---|---|
| Async runtime | `tokio` |
| EVM interaction (Hemi + Ethereum) | `alloy` (not ethers-rs) |
| Bitcoin RPC | `bitcoincore-rpc`, `bitcoin` |
| Database | `sqlx` + PostgreSQL |
| HTTP server | `axum` |
| GraphQL | `async-graphql`, `async-graphql-axum` |
| Serialization | `serde`, `serde_json` |
| Error handling | `thiserror` (libraries), `anyhow` (binary) |
| Tracing | `tracing`, `tracing-subscriber` |
| Config | `config` + `dotenvy` |

---

## Prerequisites

- **Rust** 1.75+ (2021 edition)
- **PostgreSQL** 14+
- **Bitcoin full node** (or a testnet/regtest node) with RPC enabled
- **Hemi RPC endpoint** — testnet: `https://testnet.rpc.hemi.network/rpc`
- **Ethereum RPC endpoint** — e.g. Alchemy or Infura (Sepolia for testnet)
- [`sqlx-cli`](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli) for running migrations

---

## Setup

### 1. Clone and configure

```bash
git clone https://github.com/strait-data/strait
cd strait
cp .env.example .env
```

Edit `.env` with your node URLs, credentials, and contract addresses:

```bash
# Bitcoin
BITCOIN_RPC_URL=http://localhost:8332
BITCOIN_RPC_USER=user
BITCOIN_RPC_PASSWORD=password
BITCOIN_TUNNEL_ADDRESSES=bc1q...,...   # comma-separated custody addresses to watch
BITCOIN_CONFIRMATION_DEPTH=6

# Hemi
HEMI_RPC_URL=https://testnet.rpc.hemi.network/rpc
HEMI_CHAIN_ID=743111
HEMI_TUNNEL_CONTRACT=0x...
HEMI_START_BLOCK=0
HEMI_CONFIRMATION_DEPTH=3

# Ethereum
ETH_RPC_URL=https://eth-sepolia.g.alchemy.com/v2/YOUR_KEY
ETH_CHAIN_ID=11155111
ETH_TUNNEL_CONTRACT=0x...
ETH_START_BLOCK=0
ETH_CONFIRMATION_DEPTH=12

# Database
DATABASE_URL=postgres://postgres:password@localhost:5432/strait

# API
API_HOST=0.0.0.0
API_PORT=8080
```

### 2. Create the database

```bash
createdb strait
sqlx migrate run
```

### 3. Build

```bash
cargo build --release
```

### 4. Run

```bash
cargo run --release -p strait-node
```

The node starts all three ingesters, the join engine, and the API server concurrently. Shutdown cleanly with `Ctrl+C` — all tasks drain in-flight work before exiting.

---

## API

### GraphQL

The GraphQL API is served at `http://localhost:8080/graphql`. In development, a GraphiQL playground is available at `http://localhost:8080/`.

**Query a single transfer:**

```graphql
query {
  tunnelTransfer(id: "uuid-here") {
    id
    asset
    direction
    route
    amount
    sender
    recipient
    status
    initiatedAt
    finalizedAt
    sourceTx {
      chain
      hash
      blockNumber
      confirmations
    }
    destinationTx {
      chain
      hash
      blockNumber
    }
    popProofs {
      bitcoinTxid
      bitcoinBlock
      observedAt
    }
    reorgEvents {
      chain
      depth
      affectedFromBlock
      detectedAt
    }
  }
}
```

**List transfers with filters:**

```graphql
query {
  tunnelTransfers(
    filter: {
      route: BTC_TO_HEMI
      status: FINALIZED
      amountGte: "1000000"          # in base units (satoshis / wei)
      initiatedAfter: "2025-03-12T00:00:00Z"
    }
    first: 20
    after: "cursor"
  ) {
    edges {
      node { id amount status initiatedAt }
      cursor
    }
    pageInfo { hasNextPage endCursor }
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

### Webhooks

Register a webhook to receive real-time push notifications on tunnel events:

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

Every delivery is signed with `HMAC-SHA256(secret, body)` in the `X-Strait-Signature` header. Deliveries are retried up to 5 times with exponential backoff. Failed deliveries are logged at `ERROR` level.

---

## Domain Model

The central object is `TunnelTransfer`:

```
TunnelTransfer
  id              UUID — globally unique cross-chain identifier
  asset           BTC | ETH | ERC20
  direction       IN (to Hemi) | OUT (from Hemi)
  route           BTC_TO_HEMI | HEMI_TO_BTC | ETH_TO_HEMI | HEMI_TO_ETH
  amount          BigDecimal (in base units)
  sender          source-chain address
  recipient       destination-chain address
  status          INITIATED | ANCHORED | FINALIZED | FAILED | REORGED
  initiated_at    timestamp
  finalized_at    timestamp (null until finalized)
  source_tx       ChainTransaction (chain, hash, block, confirmations, timestamp)
  destination_tx  ChainTransaction (null until finalized)
  pop_proofs      [] PoP miner submissions anchoring this transfer (BTC routes)
  reorg_events    [] full audit log of any reorgs that touched this transfer
```

### Transfer Lifecycle

```
Bitcoin deposit observed
  → INITIATED   (record created immediately)
  → wait for Hemi TunnelMint with matching source_txid

Hemi TunnelMint with matching source_txid observed
  → ANCHORED

PoP proof covering the Hemi mint block observed
  → FINALIZED

Bitcoin reorg covering the deposit block
  → REORGED    (retraction emitted; record preserved for audit)

Hemi reorg covering the mint block
  → back to INITIATED (mint must be re-observed)
```

---

## Reorg Safety

Reorgs are treated as normal control flow, not edge cases:

- Each ingester maintains a rolling window of recent block hashes beyond its confirmation depth
- On each new block, the parent hash is verified against the stored hash for the prior block
- A mismatch triggers a backward walk to find the fork point, emitting a `BlockReorg` event
- The join engine consumes `BlockReorg` events and emits retraction updates for any in-flight transfers that referenced reorged transactions
- Retractions are stored as updates with `reorg_at` set — records are never hard-deleted

Bitcoin has the deepest reorg window (default: 6 confirmations). Hemi uses 3. Ethereum uses 12. All are configurable.

---

## Configuration Reference

| Variable | Default | Description |
|---|---|---|
| `BITCOIN_RPC_URL` | — | Bitcoin node RPC URL |
| `BITCOIN_RPC_USER` | — | RPC username |
| `BITCOIN_RPC_PASSWORD` | — | RPC password |
| `BITCOIN_TUNNEL_ADDRESSES` | — | Comma-separated tunnel custody addresses |
| `BITCOIN_CONFIRMATION_DEPTH` | `6` | Blocks before a Bitcoin event is considered stable |
| `HEMI_RPC_URL` | — | Hemi RPC endpoint |
| `HEMI_CHAIN_ID` | `43111` (mainnet) | Hemi chain ID (743111 for testnet) |
| `HEMI_TUNNEL_CONTRACT` | — | Hemi tunnel contract address |
| `HEMI_START_BLOCK` | `0` | Block to begin indexing from |
| `HEMI_CONFIRMATION_DEPTH` | `3` | Blocks before a Hemi event is considered stable |
| `ETH_RPC_URL` | — | Ethereum RPC endpoint |
| `ETH_CHAIN_ID` | `1` (mainnet) | Ethereum chain ID |
| `ETH_TUNNEL_CONTRACT` | — | Ethereum-side tunnel contract address |
| `ETH_START_BLOCK` | `0` | Block to begin indexing from |
| `ETH_CONFIRMATION_DEPTH` | `12` | Blocks before an Ethereum event is considered stable |
| `DATABASE_URL` | — | PostgreSQL connection string |
| `API_HOST` | `0.0.0.0` | API bind address |
| `API_PORT` | `8080` | API bind port |

---

## Development

### Run tests

```bash
# Unit tests (no external dependencies)
cargo test -p strait-core
cargo test -p strait-join

# Integration tests (require a running Postgres)
DATABASE_URL=postgres://... cargo test -p strait-store
```

### Structured logging

Strait emits JSON-structured logs in production and pretty-printed logs in development. Control verbosity via `RUST_LOG`:

```bash
RUST_LOG=strait=debug,sqlx=warn cargo run -p strait-node
```

Every significant state transition (transfer created, status changed, reorg detected) emits a structured `tracing` event with the transfer ID, chain, block number, and new status. Spans correlate events across tasks for the same transfer.

### Database migrations

```bash
# Apply all pending migrations
sqlx migrate run

# Revert the last migration
sqlx migrate revert

# Add a new migration
sqlx migrate add <name>
```

---

## Open Items

The following require contract addresses from Hemi's deployed infrastructure before the corresponding code is fully operational. Stubbed with config values pending confirmation:

1. **Hemi tunnel contract address** — mainnet and testnet, from `https://explorer.hemi.xyz` or Hemi's GitHub
2. **Hemi tunnel contract ABI** — specifically the `TunnelMint` and `TunnelBurn` event signatures
3. **Bitcoin OP_RETURN encoding** — how the Hemi tunnel encodes the destination Hemi address in Bitcoin deposit transactions
4. **PoP proof contract** — which Hemi contract emits PoP proof events and its event signature
5. **Ethereum tunnel contract** — address and ABI for the Ethereum-side lock/release contract

These are marked `// FIXME: confirm with Hemi docs` in the relevant source files.

---

## License

MIT — see [LICENSE](LICENSE) for details.

---

## Related Documents

- [`files (3)/01-strait-tunnel-indexer.md`](files%20(3)/01-strait-tunnel-indexer.md) — product specification and rationale
- [`files (3)/02-strait-platform.md`](files%20(3)/02-strait-platform.md) — long-term platform vision
- [`files (3)/03-strait-wedge-to-platform-strategy.md`](files%20(3)/03-strait-wedge-to-platform-strategy.md) — sequencing strategy
