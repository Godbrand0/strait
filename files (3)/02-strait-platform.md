# Strait: The Bitcoin-Aware Data Platform

**Whitepaper v0.1 — Platform Vision**

> Real-time, reorg-safe data infrastructure for Hemi and the broader Bitcoin-aware ecosystem. Built around a primitive that no other indexer understands: native joins between EVM state and Bitcoin state.

---

## 1. Abstract

Bitcoin-aware execution environments are emerging as a distinct category of blockchain — chains where smart contracts read Bitcoin state natively rather than relying on bridges or oracles. Hemi is the largest of these, with over $1.2B in TVL and a Bitcoin full node embedded inside its EVM. As this category grows, it exposes a structural gap in data infrastructure: existing indexers are built for chains whose execution is self-contained, and they cannot represent computation that reaches into Bitcoin.

Strait is a data platform built for this new shape. Its core thesis is that Bitcoin-aware execution requires Bitcoin-aware indexing — not as a feature, but as a foundational primitive that shapes the entire system. Strait begins with Hemi and a focused wedge product (tunnel indexing), then expands into a general-purpose data layer for the full chain and adjacent Bitcoin-aware networks.

This document specifies the long-term vision, the technical primitives that make Strait differentiable, and the business model that makes it sustainable.

---

## 2. The Category Problem

### 2.1 What "Bitcoin-aware" actually means

A Bitcoin-aware chain is one where smart contracts can read Bitcoin state — UTXOs, block headers, transactions, scripts, inscriptions — without trusting an external relayer. Hemi achieves this by embedding a full Bitcoin node inside its EVM, exposing reads through precompiled contracts. Stacks achieves it differently, through Clarity's `get-burn-block-info?` and related primitives. Future chains (Botanix, Citrea, BOB) will achieve it through their own mechanisms.

What unites the category is a single fact: **the meaningful state of an application can live partly on the Bitcoin chain and partly on the host chain, joined at execution time**. A loan position on Hemi might be collateralized by an inscription identified by a specific Bitcoin UTXO. A derivatives contract might settle based on Bitcoin hashrate or fee rates derived from block headers. A tunnel transfer is defined by events on two chains plus PoP anchoring on a third.

### 2.2 Why existing indexers cannot represent this

Indexers built for self-contained execution chains (Ethereum and all EVM L2s, Solana, Sui) operate on a simple model: the chain is the source of truth, every relevant event is in the chain's logs, and indexing means extracting those logs into a queryable form.

This model breaks on Bitcoin-aware chains in three ways.

**First**, the EVM event log does not capture the Bitcoin reads that informed the contract's execution. A contract that calls `bitcoinUtxoExists(txid, vout)` and branches on the result produces an EVM trace that shows the branch taken, but not the Bitcoin data that drove the branch. The Bitcoin half of the application's state is invisible to any indexer that only looks at EVM logs.

**Second**, the relevant data lives on chains with different consensus models and finality semantics. Bitcoin's probabilistic finality, Hemi's PoP-anchored finality, and Ethereum's epoch-based finality all need to be reconciled into a single coherent view. No existing indexer handles multi-chain finality reconciliation as a core primitive — they treat each chain as independent and let users handle the join themselves.

**Third**, the meaningful identifiers for cross-chain state do not share a namespace. A Bitcoin transaction is identified by a txid hash. The Hemi contract that read it identifies it by the txid as a `bytes32`, but also by the *block height* it was confirmed at and the *witness data* it carried. The join key depends on what the contract actually used. A general indexer has no way to know what to extract.

### 2.3 The opportunity

The set of Bitcoin-aware chains is small today but growing. The category has crossed the credibility threshold — Hemi alone has institutional adoption, $1.2B TVL, and a mature ecosystem. As more chains in this category mature, every one of them will need data infrastructure that understands their shape. The first credible provider becomes the default. The default becomes the standard.

Strait's positioning is to be the standard for Bitcoin-aware data infrastructure, starting with Hemi.

---

## 3. The Platform

### 3.1 Architecture overview

Strait is composed of layered subsystems, each independently useful and collectively coherent.

```
┌───────────────────────────────────────────────────────────────┐
│  Applications, dashboards, agents, compliance tools           │
└───────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────┐
│  Surfaces:  GraphQL  │  Webhooks  │  Mirror Pipelines  │ RPC  │
└───────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────┐
│  Query layer:  Postgres (hot)  │  ClickHouse (cold)           │
└───────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────┐
│  Materialization layer:  Subgraphs  │  Custom datasets        │
└───────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────┐
│  Join engine:  Multi-chain state machine, reorg-aware         │
└───────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────┐
│  Ingestion layer:  Bitcoin │ Hemi │ Ethereum │ future chains  │
└───────────────────────────────────────────────────────────────┘
```

The platform builds upward from a small set of well-defined primitives. The ingestion layer is chain-specific. Everything above it operates on a unified internal event model.

### 3.2 The hVM-aware indexing primitive

The defining technical primitive of Strait is the ability to index a Hemi contract's execution along with the Bitcoin reads it performed during that execution.

When a Hemi contract calls an hBK precompile to check a Bitcoin UTXO, query a transaction, or read a block header, that call is invisible to a standard EVM event log. Strait captures it by running an instrumented Hemi node — or by parsing the deterministic precompile-call patterns from Hemi traces — and emitting synthetic events that represent the Bitcoin reads as first-class indexable data.

The result is a join model where an application developer can write a single subgraph query like:

```graphql
query {
  lendingPositions(where: { status: ACTIVE }) {
    borrower
    debtAmount
    collateral {
      type
      bitcoinUtxo {
        txid
        vout
        amount
        currentlySpent           # ← Bitcoin state, joined natively
        spendingTransaction      # ← Bitcoin tx that spent it, if any
      }
    }
    liquidationRisk             # ← computed from EVM + BTC state
  }
}
```

No existing indexer can produce that response. Strait can, because it understands the contract's hBK calls and joins them to the Bitcoin state that backed them.

This primitive is the moat. Everything else in the platform is execution.

### 3.3 Surfaces

Strait exposes data through four surfaces, each optimized for a distinct use case.

**Strait Subgraphs.** GraphQL APIs for app developers. Schema-driven, hot-reload-friendly, compatible mental model with The Graph for migration. Differentiated by native support for the hVM-aware primitives — `bitcoinUtxo`, `inscription`, `tunnelTransfer`, `popProof` are first-class types in the Strait subgraph DSL.

**Strait Streams.** Real-time webhooks and event subscriptions. Sub-second latency, at-least-once delivery, configurable filters, dead-letter handling. Designed for bots, agents, alerting systems, and reactive applications. Filters operate on the joined data model — "alert me when a Bitcoin UTXO collateralizing more than $1M of Hemi loans is spent" is a single filter, not a multi-system orchestration.

**Strait Mirror.** Continuous replication of customer-selected datasets into the customer's own infrastructure: Postgres, ClickHouse, S3, Kafka, BigQuery. Reorg-aware updates that retract and replay affected records. For analytics teams, compliance providers, and institutional users who need to colocate Hemi data with their own.

**Strait RPC.** Multi-region, load-balanced RPC access to Hemi nodes with built-in failover and cross-node consensus checks. Not a differentiator in itself, but a natural product extension that competitors offer and customers expect from a serious data platform.

### 3.4 Datasets

In addition to customer-defined subgraphs, Strait publishes a set of canonical datasets — pre-built, continuously updated, queryable through any surface.

- **Tunnels** — all tunnel transfers across all routes (the wedge product, expanded)
- **PoP Anchoring** — every PoP miner submission, its Bitcoin commitment, and the Hemi block range it anchored
- **Inscriptions** — Bitcoin inscriptions referenced by Hemi contracts, with current UTXO status and Hemi-side references
- **Tunneled Asset Flows** — derived dataset tracking where tunneled BTC and ETH actually go after entering Hemi (which protocols, which addresses, how long they sit)
- **hVM Bitcoin Reads** — every hBK precompile call across the chain, with the Bitcoin data it returned and the Hemi tx that triggered it
- **Bitcoin Fee and Hashrate** — clean time-series derived from Bitcoin headers, ready for use in derivative protocols and dashboards

These datasets are the platform's public surface. They are free to query within free-tier limits and form the basis of the network effect — once dashboards, wallets, and protocols depend on them, switching costs are high.

---

## 4. Why Strait Wins the Category

### 4.1 Against The Graph

The Graph is decentralized, community-driven, and present on Hemi. Its strength is decentralization and a familiar developer experience. Its weakness is that decentralization makes hVM-aware indexing prohibitively complex: every indexer in the network would need to run a Hemi node *and* understand hBK precompile semantics *and* agree on the canonical interpretation of Bitcoin state at every block. The Graph's protocol is not designed for that level of cross-chain coordination.

Strait operates a centralized service (with all the trust assumptions that implies) in exchange for the ability to ship hVM-aware indexing in months rather than years. The trade-off is the right one for the current category maturity. As the category grows and the protocols stabilize, parts of Strait may migrate to The Graph network or a similar decentralized layer — but only after the patterns are proven.

### 4.2 Against Goldsky

Goldsky is the obvious competitor in the *category* of managed real-time indexing. They have a strong product, broad chain support, real customers, and engineering depth. They are not on Hemi.

Strait's bet: Goldsky will integrate Hemi eventually, but they will integrate it as one of 150 chains, not as a core focus. They will support standard EVM indexing on Hemi. They will not build hVM-aware primitives, because doing so for one chain doesn't justify the engineering investment when their product surface is general-purpose.

Strait wins by depth-on-one-thing. When a Hemi developer asks "how do I index my BTC-collateralized lending protocol", the answer is Strait, because Strait is the only platform that natively models the joined state.

### 4.3 Against custom in-house solutions

The largest Hemi protocols and the foundation itself will continue to maintain some internal data infrastructure for their own needs. Strait does not displace this — it complements it. The internal team handles bespoke business logic; Strait handles the canonical chain state that the internal team would otherwise spend engineering hours replicating.

The pitch to in-house teams is straightforward: stop maintaining the bottom of your data stack. Use Strait for the canonical Hemi + Bitcoin state. Spend your engineering time on the parts that are actually proprietary to your product.

---

## 5. Business Model

### 5.1 Revenue lines

- **Self-serve subscriptions** — tiered pricing on query volume, webhook throughput, and Mirror data egress. The dominant revenue line at maturity.
- **Enterprise contracts** — dedicated infrastructure, SLAs, compliance attestations, custom datasets. Smaller in count but disproportionate in revenue per customer.
- **Foundation and ecosystem grants** — recurring infrastructure grants from Hemispheres Foundation and analogous bodies on other chains Strait integrates. Significant in early years, declining as a percentage of revenue over time.
- **Premium datasets** — paid access to derived datasets that require non-trivial computation (MEV opportunity feeds, large-flow alerts, predictive models). Optional and additive.

### 5.2 Unit economics

The economics of indexing infrastructure are well-understood from comparable companies (Goldsky, Alchemy, QuickNode). Gross margins of 70–85% are typical once scale is reached, driven primarily by infrastructure cost amortization across customers. Customer acquisition cost is low for infrastructure products because adoption is driven by developer documentation and community presence rather than sales motion. Net revenue retention exceeds 130% in healthy comparable companies because customers grow into higher tiers as their products grow.

Strait's wedge-then-expand strategy means the platform reaches positive unit economics with relatively few customers — the fixed infrastructure cost for a single chain is modest, and the tunnel wedge has paying customers from month six.

### 5.3 Long-term moat

The moat is data primacy, not technology. Three years in:

- Strait's datasets are referenced by every analytics platform covering Hemi
- Strait's webhook integrations are embedded in every major wallet, bot, and DeFi protocol on the chain
- Strait's hVM-aware subgraph DSL is the de facto standard for Bitcoin-aware indexing
- Strait operates equivalent infrastructure on Botanix, Citrea, BOB, and other Bitcoin-aware chains

A competitor entering at year three faces not just a product gap but a documentation gap, a tutorial gap, an integration gap, and a trust gap. The moat compounds.

---

## 6. Multi-Chain Expansion

### 6.1 Why Bitcoin-aware chains specifically

Strait does not aspire to be a generalist indexer competing with Goldsky and Alchemy across hundreds of chains. The category bet is Bitcoin-aware execution. The chains that matter in this category are small in number and large in importance.

- **Hemi** — primary platform (year 1+)
- **Botanix** — EVM-equivalent Bitcoin L2 with merge-mining (evaluation year 2)
- **Citrea** — Bitcoin ZK rollup (evaluation year 2)
- **BOB** — Bitcoin-Optimism integration (evaluation year 2)
- **Stacks** — Bitcoin-aware via Clarity (evaluation year 2-3, schema model is different but the join primitive applies)
- **Future** — any chain where smart contracts read Bitcoin state natively

Each integration leverages the same join engine, the same Bitcoin ingestion infrastructure, and the same surfaces. The marginal cost of adding a chain is the chain-specific ingestion plus schema adaptation, not a full platform rebuild.

### 6.2 The cross-chain Bitcoin index

A second-order opportunity emerges from multi-chain coverage: Strait becomes the only platform with a unified view of Bitcoin state as referenced across *all* Bitcoin-aware chains. A specific BTC UTXO might be collateralizing a loan on Hemi, referenced in a Citrea contract, and tracked by a Stacks application — all at once. Strait sees all of it.

This is genuinely novel data. No one else has it. It opens product surfaces that don't exist today: cross-chain Bitcoin asset tracking, multi-chain compliance reporting, arbitrage opportunities between Bitcoin-aware L2s. The platform thesis is that this becomes increasingly valuable as the category matures.

---

## 7. Risks

**Hemi ecosystem stalls.** Strait's near-term revenue depends on Hemi's adoption curve continuing. If TVL stagnates or the chain loses developer mindshare, Strait's growth caps out. Mitigation: multi-chain expansion sequenced to begin once Hemi's revenue line is established but before it plateaus.

**Goldsky enters Hemi aggressively.** A well-funded incumbent with broader chain coverage could price below Strait's tiers and bundle Hemi access with their existing customer base. Mitigation: depth on hVM-aware primitives that Goldsky cannot match without a focused investment in a single chain. Maintain a price gap. Lock in foundation and major-protocol relationships early.

**Indexer commoditization.** A long-tail risk that managed indexing becomes a low-margin commodity service over a five-year horizon. Mitigation: derived datasets and premium analytics shift revenue mix toward higher-margin products. The cross-chain Bitcoin index is structural insulation.

**Hemi consensus or PoP changes break ingestion.** Hemi is a young chain. Protocol changes (V2 upgrade, future versions) could require significant re-engineering of the ingester and join engine. Mitigation: close foundation relationship, advance visibility into protocol changes, modular architecture that isolates chain-specific code.

---

## 8. Three-Year Trajectory

**Year 1 — Wedge.** Tunnel indexer ships, reaches public launch, signs the first 50 paying customers. Revenue is small but proves the business. Strait becomes known in the Hemi ecosystem.

**Year 2 — Platform.** Expansion to general hVM-aware indexing. All four surfaces (Subgraphs, Streams, Mirror, RPC) shipped. The hVM-aware indexing primitive becomes the platform's defining feature. Foundation grant transitions from project funding to ecosystem partnership. Revenue runs at $1.5M–$3M ARR.

**Year 3 — Category leader.** Multi-chain expansion begins. Strait is the canonical data layer for Hemi and the first credible provider for the broader Bitcoin-aware category. Cross-chain Bitcoin index launches. Revenue at $5M–$10M ARR depending on multi-chain timing. Strait raises a meaningful Series A or chooses to remain bootstrapped on revenue.

The trajectory is ambitious. It is also grounded in a specific, defensible thesis: Bitcoin-aware execution is a real category, it needs data infrastructure built for its shape, and the first credible provider becomes the standard.

---

*Document version 0.1. This is the long-horizon vision. The path to it begins with the wedge described in* Strait: The Tunnel Indexer *and is governed by the sequencing in* Strait: Wedge to Platform Strategy.
