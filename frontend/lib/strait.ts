// Strait GraphQL client + view helpers (server-side).

export const STRAIT_API_URL =
  process.env.STRAIT_API_URL ?? "http://localhost:8080/graphql";

// Block-explorer bases. Default to mainnet (the node's default target); override
// via env for testnet, e.g. NEXT_PUBLIC_HEMI_EXPLORER=https://testnet.explorer.hemi.xyz
const HEMI_EXPLORER =
  process.env.NEXT_PUBLIC_HEMI_EXPLORER ?? "https://explorer.hemi.xyz";
const BTC_EXPLORER =
  process.env.NEXT_PUBLIC_BTC_EXPLORER ?? "https://mempool.space";
const ETH_EXPLORER =
  process.env.NEXT_PUBLIC_ETH_EXPLORER ?? "https://etherscan.io";

export type Transfer = {
  id: string;
  asset: string;
  direction: string;
  route: string;
  amount: string;
  sender: string;
  recipient: string;
  status: string;
  sourceChain: string;
  sourceTxHash: string;
  sourceBlock: number;
  sourceTimestamp: string;
  destChain: string | null;
  destTxHash: string | null;
  destBlock: number | null;
  popAnchored: boolean;
  popKeystoneBlock: number | null;
  popScore: number | null;
  popAnchoredAt: string | null;
  initiatedAt: string;
  finalizedAt: string | null;
};

export type Stats = { totalTransfers: number };

const TRANSFER_FIELDS = `
  id asset direction route amount sender recipient status
  sourceChain sourceTxHash sourceBlock sourceTimestamp
  destChain destTxHash destBlock
  popAnchored popKeystoneBlock popScore popAnchoredAt
  initiatedAt finalizedAt
`;

/** Run a GraphQL query against the Strait node. Returns null on any failure so
 *  pages can render an "indexer offline" state instead of crashing. */
export async function graphql<T>(
  query: string,
  variables?: Record<string, unknown>,
): Promise<T | null> {
  try {
    const res = await fetch(STRAIT_API_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ query, variables }),
      cache: "no-store",
    });
    if (!res.ok) return null;
    const json = await res.json();
    if (json.errors) {
      console.error("GraphQL errors:", json.errors);
      return null;
    }
    return json.data as T;
  } catch (e) {
    console.error("Strait API unreachable:", e);
    return null;
  }
}

export async function getOverview(): Promise<{
  stats: Stats;
  transfers: Transfer[];
} | null> {
  return graphql(`{
    stats { totalTransfers }
    transfers(limit: 50) { ${TRANSFER_FIELDS} }
  }`);
}

export async function getTransfer(id: string): Promise<Transfer | null> {
  const data = await graphql<{ transfer: Transfer | null }>(
    `query($id: UUID!) { transfer(id: $id) { ${TRANSFER_FIELDS} } }`,
    { id },
  );
  return data?.transfer ?? null;
}

// ── View helpers ─────────────────────────────────────────────────────────────

export const STATUSES = ["INITIATED", "ANCHORED", "FINALIZED", "FAILED", "REORGED"] as const;

export function statusStyle(status: string): { dot: string; text: string; label: string } {
  switch (status) {
    case "FINALIZED":
      return { dot: "bg-emerald-400", text: "text-emerald-300", label: "Finalized" };
    case "ANCHORED":
      return { dot: "bg-orange-400", text: "text-orange-300", label: "Anchored" };
    case "INITIATED":
      return { dot: "bg-zinc-400", text: "text-zinc-300", label: "Initiated" };
    case "FAILED":
      return { dot: "bg-red-500", text: "text-red-400", label: "Failed" };
    case "REORGED":
      return { dot: "bg-red-500", text: "text-red-400", label: "Reorged" };
    default:
      return { dot: "bg-zinc-500", text: "text-zinc-400", label: status };
  }
}

/** Deposit (into Hemi) vs Withdrawal (out of Hemi), from the transfer direction. */
export function transferKind(direction: string): { label: string; hint: string; cls: string } {
  return direction === "OUT"
    ? { label: "Withdrawal", hint: "out of Hemi", cls: "text-sky-300 border-sky-400/30" }
    : { label: "Deposit", hint: "into Hemi", cls: "text-violet-300 border-violet-400/30" };
}

export function routeLabel(route: string): string {
  const map: Record<string, string> = {
    BTC_TO_HEMI: "BTC → Hemi",
    HEMI_TO_BTC: "Hemi → BTC",
    ETH_TO_HEMI: "ETH → Hemi",
    HEMI_TO_ETH: "Hemi → ETH",
  };
  return map[route] ?? route;
}

function decimalsFor(asset: string): number {
  if (asset === "BTC") return 8;
  return 18; // ETH and ERC-20 default
}

/** Format an atomic-unit amount string into a human value with its asset. */
export function formatAmount(asset: string, amount: string): string {
  const decimals = decimalsFor(asset);
  let v: bigint;
  try {
    v = BigInt(amount);
  } catch {
    return `${amount} ${asset}`;
  }
  const base = BigInt(10) ** BigInt(decimals);
  const whole = v / base;
  const frac = v % base;
  if (frac === BigInt(0)) return `${whole} ${asset}`;
  // up to 6 significant fractional digits, trimmed
  let fracStr = frac.toString().padStart(decimals, "0").slice(0, 6).replace(/0+$/, "");
  return fracStr ? `${whole}.${fracStr} ${asset}` : `${whole} ${asset}`;
}

export function shortHash(hash: string | null | undefined, lead = 8, tail = 6): string {
  if (!hash) return "—";
  if (hash.length <= lead + tail + 1) return hash;
  return `${hash.slice(0, lead)}…${hash.slice(-tail)}`;
}

export function timeAgo(iso: string | null | undefined): string {
  if (!iso) return "—";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "—";
  const s = Math.max(0, Math.floor((Date.now() - then) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

/** Whether the source leg is a real, observed transaction. BTC carries a real
 *  txid; an EVM source is only real once we've seen its block. ETH→Hemi currently
 *  mirrors the Hemi mint hash as a placeholder until the L1 deposit is matched,
 *  so a zero source block means "not yet observed" — don't link it. */
export function sourceObserved(t: Transfer): boolean {
  return t.sourceChain === "BITCOIN" || t.sourceBlock > 0;
}

/** Build an explorer link for a tx on a given chain ("BITCOIN" | "HEMI" | "ETHEREUM"). */
export function txExplorerUrl(chain: string | null, hash: string | null): string | null {
  if (!hash) return null;
  switch (chain) {
    case "BITCOIN":
      return `${BTC_EXPLORER}/tx/${hash.replace(/^0x/, "")}`;
    case "HEMI":
      return `${HEMI_EXPLORER}/tx/${hash}`;
    case "ETHEREUM":
      return `${ETH_EXPLORER}/tx/${hash}`;
    default:
      return null;
  }
}
