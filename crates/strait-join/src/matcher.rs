//! Cross-chain event matcher.
//!
//! Matches raw ingestion events across chains to build TunnelTransfers.
//!
//! Matching rules (from prompt.txt):
//! - BTC → Hemi: Match `BitcoinEvent::TunnelDeposit` with `HemiEvent::TunnelMint`
//!   (linked via `source_txid` on the mint, or by address+amount+time window)
//! - ETH → Hemi: Match `EthereumEvent::TunnelLock` with `HemiEvent::TunnelRelease`
//!   (linked by address+amount+time window)

use std::collections::HashMap;
use chrono::{DateTime, Utc};
use strait_core::{
    events::{BitcoinEvent, EthereumEvent, HemiEvent, RawEvent},
    types::BitcoinTxid,
};

/// Configuration for matching behavior.
#[derive(Debug, Clone)]
pub struct MatcherConfig {
    /// Maximum age gap between source and destination events (in seconds).
    pub max_event_gap_secs: i64,
    /// Tolerance for amount matching (fraction, e.g. 0.01 for 1%).
    pub amount_tolerance: f64,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            max_event_gap_secs: 3600, // 1 hour
            amount_tolerance: 0.01,   // 1%
        }
    }
}

/// Key for indexing pending events.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct MatchKey {
    /// The destination address (where value is sent on the source chain).
    address: String,
    /// The asset type (e.g. "BTC", "ETH", "USDC").
    asset: String,
}

/// A pending event waiting to be matched.
#[derive(Debug, Clone)]
struct PendingEvent {
    raw: RawEvent,
    address: String,
    asset: String,
    amount: f64,
    timestamp: DateTime<Utc>,
    /// For BTC deposits: the txid that the Hemi mint may reference.
    source_txid: Option<BitcoinTxid>,
}

/// Result of a successful match.
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub source: RawEvent,
    pub destination: RawEvent,
    pub direction: MatchDirection,
    pub amount: f64,
    pub address: String,
}

/// Which direction the matched transfer flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchDirection {
    /// Bitcoin → Hemi (deposit on BTC, mint on Hemi)
    BtcToHemi,
    /// Ethereum → Hemi (lock on ETH, release on Hemi)
    EthToHemi,
}

/// Matcher that pairs source-chain and destination-chain events.
pub struct EventMatcher {
    config: MatcherConfig,
    /// Pending BTC deposits waiting for a Hemi mint.
    pending_btc_deposits: HashMap<MatchKey, Vec<PendingEvent>>,
    /// Pending ETH locks waiting for a Hemi release.
    pending_eth_locks: HashMap<MatchKey, Vec<PendingEvent>>,
    /// Pending Hemi mints waiting for a BTC deposit.
    pending_hemi_mints: HashMap<BitcoinTxid, Vec<PendingEvent>>,
    /// Pending Hemi mints keyed by address+asset (fallback when no source_txid).
    pending_hemi_mints_by_addr: HashMap<MatchKey, Vec<PendingEvent>>,
    /// Pending Hemi releases waiting for an ETH lock.
    pending_hemi_releases: HashMap<MatchKey, Vec<PendingEvent>>,
}

impl EventMatcher {
    pub fn new(config: MatcherConfig) -> Self {
        Self {
            config,
            pending_btc_deposits: HashMap::new(),
            pending_eth_locks: HashMap::new(),
            pending_hemi_mints: HashMap::new(),
            pending_hemi_mints_by_addr: HashMap::new(),
            pending_hemi_releases: HashMap::new(),
        }
    }

    /// Process a raw event and return a match if one is found.
    pub fn process_event(&mut self, event: RawEvent) -> Option<MatchResult> {
        match event {
            RawEvent::Bitcoin(ref btc) => self.process_btc_event(event, btc),
            RawEvent::Hemi(ref hemi) => self.process_hemi_event(event, hemi),
            RawEvent::Ethereum(ref eth) => self.process_eth_event(event, eth),
        }
    }

    fn process_btc_event(&mut self, event: RawEvent, btc: &BitcoinEvent) -> Option<MatchResult> {
        match btc {
            BitcoinEvent::TunnelDeposit {
                to_address,
                amount_sats,
                txid,
                block_time,
                ..
            } => {
                let pending = PendingEvent {
                    raw: event.clone(),
                    address: to_address.to_string(),
                    asset: "BTC".to_string(),
                    amount: *amount_sats as f64,
                    timestamp: *block_time,
                    source_txid: Some(txid.clone()),
                };

                // Try to match against a pending Hemi mint that references this txid
                if let Some(mints) = self.pending_hemi_mints.get_mut(txid) {
                    if let Some(pos) = mints.iter().position(|m| {
                        self.amounts_match(pending.amount, m.amount)
                    }) {
                        let mint = mints.remove(pos);
                        if mints.is_empty() {
                            self.pending_hemi_mints.remove(txid);
                        }
                        return Some(MatchResult {
                            source: pending.raw,
                            destination: mint.raw,
                            direction: MatchDirection::BtcToHemi,
                            amount: pending.amount,
                            address: pending.address,
                        });
                    }
                }

                // Try address+asset+amount match
                let key = MatchKey {
                    address: to_address.to_string(),
                    asset: "BTC".to_string(),
                };
                if let Some(mints) = self.pending_hemi_mints_by_addr.get_mut(&key) {
                    if let Some(pos) = mints.iter().position(|m| {
                        self.amounts_match(pending.amount, m.amount)
                            && self.within_time_window(pending.timestamp, m.timestamp)
                    }) {
                        let mint = mints.remove(pos);
                        if mints.is_empty() {
                            self.pending_hemi_mints_by_addr.remove(&key);
                        }
                        return Some(MatchResult {
                            source: pending.raw,
                            destination: mint.raw,
                            direction: MatchDirection::BtcToHemi,
                            amount: pending.amount,
                            address: pending.address,
                        });
                    }
                }

                // No match — store for later
                self.pending_btc_deposits
                    .entry(key)
                    .or_default()
                    .push(pending);
                None
            }
            _ => None,
        }
    }

    fn process_hemi_event(&mut self, event: RawEvent, hemi: &HemiEvent) -> Option<MatchResult> {
        match hemi {
            HemiEvent::TunnelMint {
                source_txid,
                to,
                asset,
                amount,
                block_number,
                ..
            } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let pending = PendingEvent {
                    raw: event.clone(),
                    address: to.clone(),
                    asset: format!("{:?}", asset),
                    amount: amount_f64,
                    timestamp: Utc::now(), // HemiEvent doesn't carry a timestamp
                    source_txid: source_txid.clone(),
                };

                // If source_txid is present, try direct txid match
                if let Some(ref src_txid) = source_txid {
                    if let Some(deposits) = self.pending_btc_deposits_by_txid(src_txid) {
                        if let Some(pos) = deposits.iter().position(|d| {
                            self.amounts_match(d.amount, pending.amount)
                        }) {
                            let deposit = deposits.remove(pos);
                            return Some(MatchResult {
                                source: deposit.raw,
                                destination: pending.raw,
                                direction: MatchDirection::BtcToHemi,
                                amount: pending.amount,
                                address: pending.address,
                            });
                        }
                    }
                    // Store by txid for later
                    self.pending_hemi_mints
                        .entry(src_txid.clone())
                        .or_default()
                        .push(pending);
                } else {
                    // Try address+asset match
                    let key = MatchKey {
                        address: to.clone(),
                        asset: format!("{:?}", asset),
                    };
                    if let Some(deposits) = self.pending_btc_deposits.get_mut(&key) {
                        if let Some(pos) = deposits.iter().position(|d| {
                            self.amounts_match(d.amount, pending.amount)
                        }) {
                            let deposit = deposits.remove(pos);
                            if deposits.is_empty() {
                                self.pending_btc_deposits.remove(&key);
                            }
                            return Some(MatchResult {
                                source: deposit.raw,
                                destination: pending.raw,
                                direction: MatchDirection::BtcToHemi,
                                amount: pending.amount,
                                address: pending.address,
                            });
                        }
                    }
                    self.pending_hemi_mints_by_addr
                        .entry(key)
                        .or_default()
                        .push(pending);
                }
                None
            }
            HemiEvent::TunnelRelease {
                to,
                asset,
                amount,
                ..
            } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let key = MatchKey {
                    address: to.clone(),
                    asset: format!("{:?}", asset),
                };
                let pending = PendingEvent {
                    raw: event.clone(),
                    address: to.clone(),
                    asset: format!("{:?}", asset),
                    amount: amount_f64,
                    timestamp: Utc::now(),
                    source_txid: None,
                };

                // Try to match against a pending ETH lock
                if let Some(locks) = self.pending_eth_locks.get_mut(&key) {
                    if let Some(pos) = locks.iter().position(|l| {
                        self.amounts_match(l.amount, pending.amount)
                    }) {
                        let lock = locks.remove(pos);
                        if locks.is_empty() {
                            self.pending_eth_locks.remove(&key);
                        }
                        return Some(MatchResult {
                            source: lock.raw,
                            destination: pending.raw,
                            direction: MatchDirection::EthToHemi,
                            amount: pending.amount,
                            address: pending.address,
                        });
                    }
                }

                // No match — store for later
                self.pending_hemi_releases
                    .entry(key)
                    .or_default()
                    .push(pending);
                None
            }
            _ => None,
        }
    }

    fn process_eth_event(&mut self, event: RawEvent, eth: &EthereumEvent) -> Option<MatchResult> {
        match eth {
            EthereumEvent::TunnelLock {
                from,
                asset,
                amount,
                ..
            } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let key = MatchKey {
                    address: from.clone(),
                    asset: format!("{:?}", asset),
                };
                let pending = PendingEvent {
                    raw: event.clone(),
                    address: from.clone(),
                    asset: format!("{:?}", asset),
                    amount: amount_f64,
                    timestamp: Utc::now(),
                    source_txid: None,
                };

                // Try to match against a pending Hemi release
                if let Some(releases) = self.pending_hemi_releases.get_mut(&key) {
                    if let Some(pos) = releases.iter().position(|r| {
                        self.amounts_match(pending.amount, r.amount)
                    }) {
                        let release = releases.remove(pos);
                        if releases.is_empty() {
                            self.pending_hemi_releases.remove(&key);
                        }
                        return Some(MatchResult {
                            source: pending.raw,
                            destination: release.raw,
                            direction: MatchDirection::EthToHemi,
                            amount: pending.amount,
                            address: pending.address,
                        });
                    }
                }

                // No match — store for later
                self.pending_eth_locks
                    .entry(key)
                    .or_default()
                    .push(pending);
                None
            }
            _ => None,
        }
    }

    /// Helper: look up pending BTC deposits by txid.
    fn pending_btc_deposits_by_txid(&mut self, txid: &BitcoinTxid) -> Option<&mut Vec<PendingEvent>> {
        // We need to search through all pending_btc_deposits to find ones with matching source_txid.
        // This is O(n) but acceptable for the expected volume.
        for (_, deposits) in self.pending_btc_deposits.iter_mut() {
            if deposits.iter().any(|d| d.source_txid.as_ref() == Some(txid)) {
                return Some(deposits);
            }
        }
        None
    }

    /// Check if two amounts match within tolerance.
    fn amounts_match(&self, a: f64, b: f64) -> bool {
        if a == 0.0 || b == 0.0 {
            return false;
        }
        let diff = (a - b).abs();
        let tolerance = a.max(b) * self.config.amount_tolerance;
        diff <= tolerance
    }

    /// Check if two timestamps are within the configured window.
    fn within_time_window(&self, a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
        let diff = (a - b).num_seconds().abs();
        diff <= self.config.max_event_gap_secs
    }

    /// Total number of pending events across all queues.
    pub fn pending_count(&self) -> PendingCount {
        PendingCount {
            btc_deposits: self.pending_btc_deposits.values().map(|v| v.len()).sum(),
            eth_locks: self.pending_eth_locks.values().map(|v| v.len()).sum(),
            hemi_mints: self.pending_hemi_mints.values().map(|v| v.len()).sum()
                + self.pending_hemi_mints_by_addr.values().map(|v| v.len()).sum(),
            hemi_releases: self.pending_hemi_releases.values().map(|v| v.len()).sum(),
        }
    }

    /// Clear all pending events.
    pub fn clear(&mut self) {
        self.pending_btc_deposits.clear();
        self.pending_eth_locks.clear();
        self.pending_hemi_mints.clear();
        self.pending_hemi_mints_by_addr.clear();
        self.pending_hemi_releases.clear();
    }
}

/// Summary of pending event counts.
#[derive(Debug, Clone, Default)]
pub struct PendingCount {
    pub btc_deposits: usize,
    pub eth_locks: usize,
    pub hemi_mints: usize,
    pub hemi_releases: usize,
}

impl PendingCount {
    pub fn total(&self) -> usize {
        self.btc_deposits + self.eth_locks + self.hemi_mints + self.hemi_releases
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use chrono::Utc;
    use strait_core::types::{Address, Asset, BitcoinAddress, BitcoinTxid, TxHash};

    fn make_btc_deposit(txid_bytes: [u8; 32], addr: &str, amount_sats: u64) -> RawEvent {
        RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
            txid: BitcoinTxid(txid_bytes),
            vout: 0,
            to_address: BitcoinAddress::new(addr),
            amount_sats,
            op_return_data: None,
            block_number: 100,
            block_hash: strait_core::types::BlockHash([0; 32]),
            block_time: Utc::now(),
        })
    }

    fn make_hemi_mint(
        source_txid: Option<BitcoinTxid>,
        to: &str,
        amount: &str,
    ) -> RawEvent {
        RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash([0; 32]),
            asset: Asset::Btc,
            amount: BigDecimal::parse(amount).unwrap(),
            to: Address(to.to_string()),
            source_txid,
            block_number: 200,
            log_index: 0,
        })
    }

    #[test]
    fn test_btc_deposit_then_hemi_mint_with_source_txid() {
        let config = MatcherConfig::default();
        let mut matcher = EventMatcher::new(config);

        let txid = BitcoinTxid([1u8; 32]);
        let btc = make_btc_deposit([1u8; 32], "bc1qtest", 100_000_000);
        let hemi = make_hemi_mint(Some(txid.clone()), "0xbob", "100000000");

        // BTC deposit first — no match yet
        assert!(matcher.process_event(btc).is_none());
        // Hemi mint — should match
        let result = matcher.process_event(hemi);
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.direction, MatchDirection::BtcToHemi);
    }

    #[test]
    fn test_hemi_mint_first_then_btc_deposit() {
        let config = MatcherConfig::default();
        let mut matcher = EventMatcher::new(config);

        let txid = BitcoinTxid([2u8; 32]);
        let hemi = make_hemi_mint(Some(txid.clone()), "0xbob", "100000000");
        let btc = make_btc_deposit([2u8; 32], "bc1qtest", 100_000_000);

        // Hemi mint first — no match yet
        assert!(matcher.process_event(hemi).is_none());
        // BTC deposit — should match
        let result = matcher.process_event(btc);
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.direction, MatchDirection::BtcToHemi);
    }

    #[test]
    fn test_no_match_different_amounts() {
        let config = MatcherConfig {
            amount_tolerance: 0.01,
            ..Default::default()
        };
        let mut matcher = EventMatcher::new(config);

        let txid = BitcoinTxid([3u8; 32]);
        let btc = make_btc_deposit([3u8; 32], "bc1qtest", 100_000_000);
        let hemi = make_hemi_mint(Some(txid.clone()), "0xbob", "200000000");

        assert!(matcher.process_event(btc).is_none());
        // Amounts differ by 100% — no match
        assert!(matcher.process_event(hemi).is_none());
        assert_eq!(matcher.pending_count().total(), 2);
    }

    #[test]
    fn test_amount_tolerance_matching() {
        let config = MatcherConfig {
            amount_tolerance: 0.02, // 2%
            ..Default::default()
        };
        let mut matcher = EventMatcher::new(config);

        let txid = BitcoinTxid([4u8; 32]);
        let btc = make_btc_deposit([4u8; 32], "bc1qtest", 100_000_000);
        // 1% difference — within tolerance
        let hemi = make_hemi_mint(Some(txid.clone()), "0xbob", "101000000");

        assert!(matcher.process_event(btc).is_none());
        let result = matcher.process_event(hemi);
        assert!(result.is_some());
    }

    #[test]
    fn test_pending_count() {
        let config = MatcherConfig::default();
        let mut matcher = EventMatcher::new(config);

        let btc = make_btc_deposit([5u8; 32], "bc1qtest", 100_000_000);
        assert!(matcher.process_event(btc).is_none());

        let counts = matcher.pending_count();
        assert_eq!(counts.btc_deposits, 1);
        assert_eq!(counts.total(), 1);
    }
}