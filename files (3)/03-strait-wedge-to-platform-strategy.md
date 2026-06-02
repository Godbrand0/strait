# Strait: Wedge to Platform Strategy

**Whitepaper v0.1 — Strategic Sequencing**

> How a focused six-month build becomes a category-defining data platform. The thesis, the sequencing, the inflection points, and the conditions under which to push forward or hold back.

---

## 1. Abstract

Two paths exist for entering the Hemi data infrastructure market. The first is to ship a focused tunnel indexer in six months. The second is to build a general-purpose data platform over two to three years. The companion whitepapers in this set specify each as a standalone product.

This document specifies the *third* path, which is neither in isolation: ship the tunnel indexer first, use the wedge to earn the technical credibility, operational reputation, and ecosystem relationships required to expand into the platform, and let market signals determine the timing of each expansion stage.

This is the recommended path. It is not the path with the highest theoretical upside (the pure platform play has that). It is the path with the highest *risk-adjusted* upside given the constraints of a single founder building alongside other commitments, in an emerging chain category, with limited capital, against well-funded potential competitors.

This document explains why.

---

## 2. The Strategic Thesis

### 2.1 The wedge is the right product, even on its own terms

The tunnel indexer is not a stripped-down platform. It is a coherent, defensible, sellable product whose value does not depend on what comes next. If Strait shipped only the tunnel indexer and never built another product, it would still be a viable infrastructure business — small, focused, sustainable, but real.

This matters because the wedge must stand alone. A wedge that only makes sense as a stepping stone to a larger product is a roadmap, not a product. Customers buy products. Foundations fund products. The tunnel indexer must be valuable enough on its own that paying customers exist, regardless of whether the platform ever ships.

This condition is satisfied. Tunnel data has unique demand, no substitute, and willing buyers across at least seven distinct customer segments. The wedge is the right product.

### 2.2 The platform is the right destination, even from a wedge

The general-purpose platform is not a different company built on top of the wedge. It is the natural expansion path for the same infrastructure, the same join engine, the same operational competencies, and the same customer relationships developed during the wedge phase.

The platform's defining technical primitive — hVM-aware indexing with joined Bitcoin state — is required for the tunnel wedge. Tunnels *are* multi-chain state joined across Bitcoin and Hemi. Building tunnel indexing correctly forces you to build the primitive that the platform requires. The wedge is not a different problem; it is the smallest version of the platform problem.

This matters because the expansion path must be a continuation, not a pivot. A wedge that requires throwing away most of its code to become the platform is two products, not one. The tunnel indexer naturally expands into the platform because the foundational engineering is shared.

This condition is satisfied. The architecture in the tunnel indexer whitepaper is a strict subset of the architecture in the platform whitepaper. Every component built for the wedge has a role in the platform.

### 2.3 The sequencing is the right strategy

The sequencing — wedge first, platform second — is correct because it minimizes the dominant risk at each stage.

At wedge stage, the risk is *building the wrong thing*. A six-month single-developer build is short enough to ship before the market changes, focused enough to validate against real customer feedback, and small enough to fund with a single grant. The risk is not lack of ambition; the risk is committing two years to a vision that turns out to be miscalibrated to what users actually need.

At platform stage, the risk is *failing to execute against competition*. By the time Strait expands into the platform, the team has shipped a product, has paying customers, has operational reliability data, has foundation relationships, and has earned the right to raise capital or recruit talent against credible momentum. The risk is not lack of vision; the risk is lacking the credibility and resources to outrun a well-funded incumbent (Goldsky) entering the same market.

The sequencing minimizes both risks. Pure platform play maximizes the first. Pure wedge play maximizes the second.

---

## 3. The Stages

### 3.1 Stage 1 — Tunnel Indexer (months 0–6)

**Goal**: Ship a focused, production-grade tunnel indexer to public launch with three pilot customers and at least one institutional reference.

**Funding**: Single grant from Hemispheres Foundation, $40k–$80k. No equity raised.

**Team**: Single full-time engineer. Optional part-time design and documentation help.

**Surfaces shipped**: GraphQL API, webhooks. Mirror deferred to Stage 2.

**Customer count**: 3–5 pilot users, transitioning to 20–50 paying customers by month 12.

**Key technical milestones**: All three ingesters operational (Bitcoin, Hemi, Ethereum). Join engine producing TunnelTransfer records with full lifecycle. Reorg handling validated. Public dashboard live.

**Key business milestones**: Grant accepted. Pilot agreements signed. Public launch at a Hemi ecosystem event. First paying customers onboarded.

**Inflection criteria for Stage 2**: At least 20 paying customers with measurable retention. Foundation relationship producing introductions to potential partners. At least one customer requesting indexing for non-tunnel use cases (this is the strongest signal that the platform is needed).

If these criteria are not met by month 12, the answer is to optimize the wedge, not to expand. A platform built on a weak wedge fails twice.

### 3.2 Stage 2 — Adjacent Surfaces (months 6–18)

**Goal**: Extend Strait beyond tunnels into adjacent Hemi-specific datasets while keeping scope discipline.

**Funding**: Second grant or strategic ecosystem partnership ($100k–$250k range). Optionally a pre-seed round if the wedge shows strong unit economics ($500k–$1M).

**Team**: Expand to two or three engineers. First operations or DevRel hire.

**Datasets added**: PoP anchoring, hVM Bitcoin reads (the foundational hVM-aware primitive), tunneled asset flows. These are the datasets that emerge naturally from the wedge's join engine.

**Surfaces added**: Mirror pipelines (Postgres, ClickHouse, Kafka sinks).

**Customer count target**: 100–200 paying customers, average contract value rising as enterprise tier adoption begins.

**Key technical milestones**: hVM-aware subgraph DSL released. Custom dataset support shipped. First enterprise contract signed with SLA commitments.

**Inflection criteria for Stage 3**: Net revenue retention above 120%. At least one customer using Strait for production-critical workflows (DEX powering its analytics, lending protocol's positions dashboard, institutional treasury). Foundation grants transitioning from project funding to ongoing partnership.

If these criteria are met, Strait has earned the right to attempt the platform. If they are not, the right move is to deepen Stage 2 rather than rushing forward.

### 3.3 Stage 3 — Platform and Multi-Chain (months 18–36)

**Goal**: Become the category-defining data layer for Bitcoin-aware execution chains.

**Funding**: Series A or sustained bootstrap on revenue, depending on competitive intensity. Foundation grants now a smaller percentage of capital base.

**Team**: 5–10 engineers, dedicated DevRel, operations, security, sales for enterprise.

**Expansion**: First non-Hemi chain integration (likely Botanix, Citrea, or BOB depending on category traction). General-purpose subgraph platform with the hVM-aware DSL as a competitive moat. Cross-chain Bitcoin index launched.

**Customer count**: 500+ paying customers. Enterprise revenue dominant.

**Key milestones**: Multi-chain coverage operational. Strait Subgraphs becomes the default deployment target for new Hemi protocols. Recognized as the standard data layer for the Bitcoin-aware category.

This stage is the platform whitepaper's vision realized. The path here runs through Stage 1 and Stage 2, not around them.

---

## 4. Why This Sequencing Wins

### 4.1 Capital efficiency

Pure platform play requires meaningful upfront capital — a multi-year, multi-person engineering effort cannot be funded by grants alone, and raising venture capital pre-product on a thesis as specific as "data infrastructure for Bitcoin-aware chains" is difficult without proof points.

Wedge-first means the first capital infusion (the grant) is for a deliverable that exists in six months. The second capital infusion (Stage 2 grant or pre-seed) follows shipped product with paying customers. The third (Series A or sustained bootstrap) follows demonstrable traction at scale. Each capital event is justified by the prior stage's results, not by future promises.

### 4.2 Risk laddering

At Stage 1, the existential risk is small — a focused six-month build either ships or it doesn't, and either way the founder has learned a great deal about Hemi infrastructure that is reusable. Failure is recoverable.

At Stage 2, the risk is larger but informed — the team knows what works and what doesn't from Stage 1, customer needs are concrete rather than hypothesized, and the architectural patterns are proven.

At Stage 3, the risk is largest but offset by accumulated capital, talent, customer relationships, and operational maturity. By the time Strait competes on the full platform surface, it does so from a position of established credibility.

A pure platform play inverts this ordering — largest risk first, with no credibility or learning to draw on. This is sometimes the right move (when speed-to-market is the dominant constraint), but it is the wrong move here because the category is young enough that no competitor has staked a credible claim and waiting six to twelve months does not forfeit the opportunity.

### 4.3 Competitive positioning

The strongest credible threat to Strait is Goldsky entering Hemi. Goldsky's incentive to do so increases as Hemi grows. Strait must reach defensibility before that integration happens.

The wedge-first path reaches defensibility faster than the platform path. By month 12 of the wedge sequence, Strait has paying tunnel customers, foundation backing, and depth on a specific dataset that Goldsky's general-purpose platform cannot match without dedicated engineering investment. Goldsky enters with strength on generic EVM indexing; Strait owns the tunnel data and the hVM-aware primitives.

A pure platform play, by contrast, would still be building infrastructure at month 12 with no shipped product to defend. If Goldsky enters during that window, Strait is racing them on their home turf.

### 4.4 Fit with founder constraints

The founder is building alongside other commitments. A two-year, multi-person engineering project is incompatible with that constraint. A six-month focused build is achievable as a primary commitment without abandoning other workstreams.

Stage 1 can be executed by a single founder, possibly with contractor help. Stage 2 justifies hiring because there is revenue to support it. Stage 3 justifies a fundraise because there is a business to defend. The founder's bandwidth scales with the company's needs at each stage.

A pure platform play would require either abandoning other commitments at the start or building too slowly to be credible — both of which are worse outcomes than the staged path.

---

## 5. Risks Specific to the Sequencing

### 5.1 Wedge succeeds, platform never materializes

The risk that Strait ships the tunnel indexer, earns modest revenue, but never reaches the inflection point that justifies Stage 2 expansion. This is not a catastrophic outcome — Strait remains a small, profitable infrastructure business — but it is a failure to realize the larger thesis.

Mitigation: design Stage 1 with Stage 2 in mind even if Stage 2 is conditional. The join engine, schema patterns, and operational tooling built for the wedge should be the foundation for the platform. Avoid wedge-specific shortcuts that would require rework to expand. This costs perhaps 15–20% additional engineering effort in Stage 1 and dramatically reduces the cost of Stage 2 if it happens.

### 5.2 Wedge fails to ship or fails to find customers

The downside scenario. Strait builds the tunnel indexer but it never reaches public launch (technical failure) or launches without paying customers (product-market fit failure).

Mitigation: pilot customers signed before public launch (Stage 1 gate), foundation relationship established before grant deployment, regular technical milestones with foundation visibility. The biggest risk to Stage 1 is silent failure — building for months without external feedback. Avoid this by maintaining public-facing milestones from week one.

### 5.3 Goldsky enters Hemi during Stage 1

A well-funded competitor entering the same market while Strait is still building. This is the scenario that justifies the wedge-first approach (reaching defensibility faster) but it is also a real risk during the six-month build window.

Mitigation: Strait's wedge is differentiated on a specific axis (cross-chain Bitcoin awareness) that Goldsky's general-purpose integration would not match. Goldsky entering Hemi on day one with full hVM-aware indexing is the worst case and is also extremely unlikely — their pattern is broad EVM coverage, not deep chain-specific differentiation. Foundation relationships and pilot customer commitments form additional barriers.

### 5.4 The platform thesis is wrong

The largest long-horizon risk: Bitcoin-aware chains do not become a meaningful category, Hemi stagnates, no other chains in the category reach material adoption, and the platform vision is moot.

Mitigation: this is the structural reason the wedge must be valuable on its own terms. If the platform thesis fails, the wedge remains a sustainable business — small, focused, but real. The founder loses the upside of the platform vision but not the value of the work invested in the wedge.

This is also why sequencing protects against thesis risk. The wedge bets only on Hemi specifically; the platform bets on the category. Sequencing means committing to the larger bet only after the smaller bet has been validated.

---

## 6. Decision Points

The sequencing is not predetermined. Three explicit decision points govern the path.

**Decision Point 1 — End of Stage 1 (month 6).** Has the wedge shipped to public launch with at least three pilot customers and one institutional reference? If yes, proceed to Stage 2 planning. If no, optimize Stage 1 rather than expanding scope.

**Decision Point 2 — End of Stage 2 (month 18).** Does Strait have 100+ paying customers, NRR above 120%, and at least one customer treating Strait as production-critical? If yes, proceed to Stage 3 planning. If no, continue to deepen Stage 2 — there is no shame in operating a focused, profitable, single-chain infrastructure business indefinitely.

**Decision Point 3 — During Stage 3 (months 18–24).** Has at least one additional Bitcoin-aware chain reached the maturity threshold (TVL, developer adoption, foundation interest) that justifies expansion? If yes, expand multi-chain. If no, deepen Hemi differentiation and wait for the category to mature.

Each decision point is a real branch, not a checkpoint to pass through. The right answer at each branch depends on data not yet available. The strategy is the path of evaluating each decision against the available data, not the path of executing a predetermined plan.

---

## 7. Operating Principles

A small set of principles govern execution across all three stages.

**Ship and validate before expanding.** Every expansion follows shipped product with measured customer signal, not the other way around. This is the central discipline of the sequencing.

**Earn the right to grow.** Each capital event, each hire, each market expansion is justified by results from the prior stage. The growth path is not a budget to spend but a series of permissions to earn.

**Depth over breadth at each stage.** The wedge wins by depth on tunnels, not by breadth across Hemi. The platform wins by depth on hVM-aware primitives, not by breadth across chains. Premature breadth dilutes the moat at every stage.

**Foundation relationship is structural, not transactional.** Hemispheres Foundation is a long-term partner, not a grant source. Communications, milestones, visibility into roadmap changes, and reliability of execution matter more than any specific grant amount. Treat the relationship as the most valuable asset Strait has, because it is.

**Optionality is a feature, not a bug.** The sequencing preserves the option to stay small, the option to expand, and the option to pivot if the thesis breaks. Do not optimize prematurely for any specific outcome at the expense of the optionality the staged approach provides.

---

## 8. Conclusion

The case for sequencing is not that the wedge is more interesting than the platform. The platform is the larger vision and where the meaningful upside lives. The case is that the wedge is the responsible path to the platform — risk-laddered, capital-efficient, fit with founder constraints, and defensible against competition entering the market during the build window.

Ship the wedge. Earn the platform.

---

*Document version 0.1. This is the strategic frame that governs the products specified in* Strait: The Tunnel Indexer *and* Strait: The Bitcoin-Aware Data Platform. *All three documents should be read together. Strategy without product is theory. Product without strategy is execution risk. The set is the plan.*
