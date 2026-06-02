# Strait: The Tunnel Indexer

**Whitepaper v0.1 — Wedge Product**

> A purpose-built data layer for assets moving through Hemi's tunnels. Real-time, reorg-safe, cross-chain by construction.

---

## 1. Abstract

Hemi has unified Bitcoin and Ethereum into a single execution environment with over $1.2B in total value locked and 90+ deployed protocols. The most strategically important data on Hemi is not its internal EVM state — it is the *flow* of assets through its tunnels between Bitcoin, Hemi, and Ethereum. This data is currently fragmented across three chains with three different consensus models, three different finality semantics, and no canonical indexer that joins them into a coherent record.

Strait is a focused data infrastructure product that closes this gap. It indexes every tunnel event end-to-end, exposes a unified GraphQL API and real-time webhook surface, and provides analytics-grade reliability for the parties who need to see Bitcoin moving through Hemi as a single object rather than three disconnected logs.

This document specifies what Strait is, why it exists, what it ships, and how it earns its place in the Hemi ecosystem.

---

## 2. The Problem

### 2.1 What a tunnel actually is

A Hemi tunnel is a trust-minimized asset bridge between Hemi and an external chain — Bitcoin or Ethereum. A complete tunnel flow involves at least three blockchain states and often four:

1. **Initiation** on the source chain (e.g. a Bitcoin deposit to a tunnel custody address, or an Ethereum lock transaction)
2. **Anchoring / observation** — Hemi's PoP miners publish proofs to Bitcoin; relayers observe Ethereum events
3. **Crediting** on the destination chain (a Hemi mint of tunneled BTC, or an Ethereum release)
4. **Reverse path** — for withdrawals, the same dance in the opposite direction with different finality assumptions

Each step lives in a different log on a different chain. None of them carry a shared identifier in their raw form. Reconstructing "this BTC deposit became this Hemi mint became this Ethereum withdrawal" requires stateful, ordered joining across three asynchronous data sources.

### 2.2 Why existing tools fail

**General EVM indexers** (The Graph, Goldsky, Envio, Ormi) index Hemi's EVM events fine. They cannot see Bitcoin. They have no model for joining a Bitcoin UTXO consumption to the Hemi mint it triggered. A subgraph deployed on Hemi today shows you the *destination half* of every tunnel transfer with no link to its origin.

**Block explorers** show individual transactions on individual chains. They do not aggregate, do not stream, do not provide an API for cross-chain joins, and do not handle the analytical questions ("how much BTC was tunneled in the last 24h", "what is the median time-to-finality for tunnel deposits", "alert me when address X tunnels more than 10 BTC").

**Bridge analytics platforms** (LI.FI's tooling, Socket's APIs, generic bridge dashboards) treat bridges as opaque entities. They do not understand Hemi's tunneling semantics, PoP anchoring, or the dual nature of BTC tunnels vs ETH tunnels.

**Custom in-house solutions** exist at the foundation level and at a few institutional users, built in private and not exposed as products. They are also incomplete — most rely on polling and miss reorg edge cases.

The result: every team that needs canonical tunnel data is building partial versions of the same thing in isolation.

### 2.3 Who needs this data

Tunnel data is high-leverage. A small number of well-defined consumer segments are willing to pay for it.

- **Hemi Foundation** — ecosystem reporting, TVL composition, capital flow analysis, partner integration tracking
- **Analytics platforms** — Dune-equivalent dashboards on Hemi, ecosystem trackers, public TVL leaderboards
- **Compliance and AML providers** — institutional BTC flow tracking, OFAC screening of tunnel participants, regulatory reporting
- **MEV searchers and market makers** — cross-chain arbitrage detection, liquidity routing, predictive modeling of large tunnel events
- **DeFi protocols on Hemi** — knowing the origin of deposited capital, risk-stratifying lending positions by capital provenance
- **Institutional treasurers** — auditable reporting on BTC held in tunneled form, position reconciliation across chains
- **Wallets and aggregators** — surfacing tunnel status to end users, predicting completion time for in-flight transfers

Each of these is a distinct customer with distinct API needs. All of them require the same underlying joined dataset.

---

## 3. The Product

### 3.1 What Strait is

Strait is a managed indexing service for Hemi tunnel events. It ingests Bitcoin, Hemi, and Ethereum chain data; joins related events into unified tunnel transfer records; and exposes that data through GraphQL queries, real-time webhooks, and database mirror pipelines.

A consumer of Strait does not see three chains. They see tunnel transfer objects with a complete lifecycle.

### 3.2 The unified data model

The central object is the **TunnelTransfer**. Every transfer has the same shape regardless of direction or asset:

```graphql
type TunnelTransfer {
  id: ID!                          # globally unique cross-chain identifier
  asset: Asset!                    # BTC, ETH, ERC20, etc.
  direction: TunnelDirection!      # IN (to Hemi) or OUT (from Hemi)
  route: TunnelRoute!              # BTC_TO_HEMI, HEMI_TO_BTC, ETH_TO_HEMI, HEMI_TO_ETH

  amount: BigInt!
  sender: Address!                 # canonical address on source chain
  recipient: Address!              # canonical address on destination chain

  status: TunnelStatus!            # INITIATED, ANCHORED, FINALIZED, FAILED
  initiatedAt: Timestamp!
  finalizedAt: Timestamp

  sourceTx: ChainTransaction!      # full source-chain tx record
  destinationTx: ChainTransaction  # null until finalized
  popProofs: [PopProof!]!          # PoP miner submissions anchoring this transfer
  reorgEvents: [ReorgEvent!]!      # any reorgs that touched this transfer
}
```

This is the schema other systems join against. It is intentionally opinionated — direction, route, and lifecycle status are first-class fields because every downstream consumer needs them.

### 3.3 Surfaces

Strait ships three distinct API surfaces, modeled on the Goldsky split that the market has validated.

**GraphQL Subgraph API.** Standard query interface for app developers and dashboards. Supports filters, pagination, aggregations, and time-windowed queries. Compatible mental model with The Graph for teams already familiar with subgraphs.

**Webhook Streams.** Real-time push notifications for tunnel events matching configured filters. Sub-second latency from chain event to delivered webhook. Native filters for amount thresholds, address lists, asset types, and status transitions. Designed for bots, alerts, and reactive systems.

**Mirror Pipelines.** Continuous replication of tunnel data into a customer's own Postgres, ClickHouse, S3, or Kafka. Reorg-aware updates that retract and replay affected records automatically. For analytics teams that need to colocate tunnel data with their own off-chain data, or that operate at volumes where API-mediated access becomes a bottleneck.

### 3.4 What ships in v1

The minimum viable product is intentionally narrow. The bet is that depth on tunnels beats breadth across all Hemi data.

- Complete indexing of BTC→Hemi and Hemi→BTC tunnel routes
- Complete indexing of ETH→Hemi and Hemi→ETH tunnel routes
- Unified TunnelTransfer schema with full lifecycle tracking
- GraphQL API with documented schema and a hosted explorer
- Webhook delivery with at-least-once guarantees and replay support
- Bitcoin reorg handling with explicit retract semantics
- Historical backfill from Hemi mainnet genesis (March 12, 2025)
- A reference dashboard demonstrating consumer use cases

What does *not* ship in v1: Mirror pipelines (v2), arbitrary contract indexing (out of scope), price feeds (use Pyth/RedStone), MEV-specific routing tools (v3).

---

## 4. Architecture

### 4.1 Ingestion layer

Three independent chain ingestion services, one per source chain.

- **Bitcoin ingester** — connects to a Bitcoin full node, follows the chain tip with configurable confirmation depth, watches tunnel custody addresses and OP_RETURN patterns associated with Hemi tunneling, emits raw events to the internal stream.
- **Hemi ingester** — connects to a Hemi archive node, subscribes to tunnel contract events and PoP miner submissions via standard EVM log filters, emits raw events.
- **Ethereum ingester** — connects to an Ethereum archive node, watches Hemi's Ethereum-side tunnel contracts, emits raw events.

Each ingester is independently scalable and independently fails over. Their outputs are timestamped, ordered streams written to an internal log (Redpanda/Kafka-compatible).

### 4.2 Join layer

The join engine consumes the three ingestion streams and produces unified TunnelTransfer records.

Joining is non-trivial because the three streams have different latency characteristics: Bitcoin events are slow but deterministic past 6 confirmations; Hemi events are fast but their finality is anchored to Bitcoin via PoP; Ethereum events are intermediate. A naive join would either delay all records until the slowest chain is final, or emit premature records and require constant retraction.

Strait's join engine uses a state machine per tunnel route with explicit lifecycle states:

```
INITIATED → ANCHORED → FINALIZED
                    ↘ REORGED → (retraction emitted)
                    ↘ FAILED
```

Each tunnel transfer is materialized into the output dataset as soon as it reaches INITIATED, with subsequent status transitions emitted as updates. Downstream consumers receive a complete record on day one of the transfer and a stream of updates as it progresses, rather than a delayed full record.

### 4.3 Reorg handling

Bitcoin reorgs are rare but real. A tunnel transfer whose initiation transaction is reorged out must be retracted. Strait handles this explicitly:

1. Bitcoin ingester maintains a sliding window of recent blocks beyond its confirmation depth.
2. On reorg detection, the ingester emits ReorgEvent records identifying affected transactions.
3. The join engine consumes ReorgEvents and emits retraction updates for any TunnelTransfers that referenced reorged source transactions.
4. Mirror pipelines apply retractions as updates with a `reorg_at` field set, preserving audit history rather than hard-deleting.

This is the part most general indexers get wrong on Bitcoin. Strait gets it right because handling reorgs cleanly is in the critical path of the product, not an edge case.

### 4.4 Storage and serving

- **Hot path** (last 90 days): Postgres with proper indexing on the high-cardinality fields (sender, recipient, asset, status, timestamps). Sub-100ms p99 query latency target.
- **Cold path** (historical): ClickHouse or partitioned Postgres, optimized for aggregate queries over arbitrary time windows.
- **Stream backend**: Redpanda for internal event distribution and webhook fan-out.

The serving layer is stateless and horizontally scalable. GraphQL is served by an Apollo-compatible gateway; webhooks are delivered by a Rust-based dispatcher with built-in retry, backoff, and dead-letter handling.

### 4.5 Why Rust

The ingestion, join, and webhook layers are written in Rust. The reasoning is operational, not ideological: blockchain data infrastructure must be correct under load, must handle reorgs as a normal control flow rather than an exception, and must run continuously without GC pauses corrupting stream ordering. Rust's type system makes the state machine in the join engine tractable to verify by inspection, and its async runtime (Tokio) handles the I/O concurrency cleanly. The GraphQL gateway can be Node or Rust depending on team preference; the data plane is non-negotiable Rust.

---

## 5. Competitive Landscape

| | The Graph (on Hemi) | Goldsky | Custom in-house | Strait |
|---|---|---|---|---|
| Indexes Hemi EVM | ✓ | not supported | varies | ✓ |
| Indexes Bitcoin tunnel side | ✗ | ✗ | sometimes | ✓ |
| Cross-chain join | ✗ | ✗ | sometimes | ✓ |
| Real-time webhooks | ✗ | ✓ (other chains) | varies | ✓ |
| Reorg-safe by design | partial | ✓ (EVM only) | rarely | ✓ |
| Hemi-specific schema | ✗ | ✗ | varies | ✓ |
| Available as a product | ✓ | ✗ | ✗ | ✓ |

The Graph is the closest substitute and the most important one to position against. The framing is straightforward: The Graph is general-purpose EVM infrastructure that happens to support Hemi. Strait is Hemi-specific infrastructure that understands the chain's defining feature — tunnels — natively.

Goldsky is not on Hemi as of this writing. If they integrate, Strait's wedge holds because Goldsky still indexes EVM only; the cross-chain join is the differentiator.

---

## 6. Go-to-Market

### 6.1 Sequencing

1. **Foundation engagement first.** Hemi Foundation has the strongest incentive for this data to exist and the budget to fund it. Pre-grant conversation precedes the grant application.
2. **Reference customers** — sign two or three pilot users before launch: one analytics platform, one DeFi protocol on Hemi, one institutional user. Pilots are free; the goal is design feedback and case studies, not revenue.
3. **Public launch** at a Hemi ecosystem milestone (V2 mainnet upgrade, ecosystem summit, conference).
4. **Self-serve onboarding** with a free tier for hobbyist and prototype use, paid tiers scaling with query volume and webhook throughput.

### 6.2 Pricing model

- **Free tier**: 100k GraphQL queries/month, 10k webhook deliveries/month, public schema access.
- **Builder**: $99/month, 5M queries, 500k webhooks, schema customization.
- **Growth**: $499/month, 50M queries, 5M webhooks, priority support, SLA.
- **Enterprise**: custom, Mirror pipelines, dedicated infrastructure, contracted SLA, compliance attestation.

Pricing is benchmarked against Goldsky and The Graph's network pricing, deliberately set 20-30% below Goldsky on equivalent tiers to win share on a chain where they don't yet operate.

### 6.3 Distribution

Strait reaches users through Hemi documentation, ecosystem partnerships, conference presence, and integration with existing developer tools (LI.FI dashboards, wallet providers, block explorers). The team does not need a sales motion to acquire the first 50 customers — the foundation introduction is sufficient.

---

## 7. Roadmap and Milestones

**Phase 1: Foundation (months 1–2)**
- Grant proposal submitted and accepted
- Architecture finalized, infrastructure provisioned
- Bitcoin and Hemi ingesters operational on testnet

**Phase 2: Build (months 3–5)**
- All three ingesters in production
- Join engine producing TunnelTransfer records
- GraphQL API live in private beta with 2–3 pilot users
- Reorg handling validated against historical Bitcoin reorg events

**Phase 3: Launch (month 6)**
- Public launch at Hemi ecosystem event
- Webhook surface live
- Reference dashboard published
- Self-serve onboarding open

**Phase 4: Iteration (months 7–12)**
- Mirror pipelines (Postgres, Kafka)
- Custom alerting rules
- ClickHouse cold-path for historical aggregation
- First 50 paying customers

---

## 8. Funding Ask

Strait seeks an initial grant from the Hemispheres Foundation in the range of $40k–$80k to fund the wedge build through public launch. The grant covers:

- Engineering time (one full-time senior engineer for six months)
- Infrastructure (Bitcoin and Ethereum archive nodes, Redpanda cluster, Postgres, hosting)
- Security review of the join engine and webhook authentication
- Public launch materials and documentation

Beyond the grant, Strait is structured as a sustainable business. Revenue from the Growth and Enterprise tiers is projected to cover operating costs by month 12 at conservative customer assumptions (20 paid customers averaging $300/month).

The grant is not a subsidy. It is seed capital for infrastructure the ecosystem materially needs, repaid through the public good of Hemi developers having canonical tunnel data and the ecosystem-level value of a healthy data layer.

---

## 9. Why This Wins

Three reasons, in order of importance.

**The data does not exist anywhere else in joined form.** Strait is not a better indexer in a crowded market. It is the only indexer for a category of data that every serious Hemi participant needs and that no other system produces.

**The wedge is narrow enough to ship.** A six-month, single-developer build for a focused product is a credible plan. A general indexing platform for Hemi is a multi-year, multi-person undertaking and would lose a race against Goldsky if they decide to enter. Tunnels are defensible because they require specific Hemi architectural knowledge that takes weeks to acquire — not years, but enough to deter a generalist competitor from prioritizing it.

**The wedge has a natural growth path.** Tunnel indexing leads directly into general hVM-aware indexing, then into a full data platform. The wedge is not a dead-end product. It is the beachhead. (See: *Strait: Wedge to Platform Strategy*.)

---

*Document version 0.1. Authored as a working specification, not a marketing artifact. Subject to revision based on foundation feedback, pilot user requirements, and technical discovery during build.*
