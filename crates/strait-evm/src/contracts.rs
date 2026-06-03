//! Tunnel contract ABIs and log decoders.
//!
//! Two bridge mechanisms are in play:
//!
//! - **ETH/ERC-20 routes** — OP Stack L2StandardBridge at the well-known
//!   address 0x4200000000000000000000000000000000000010 on Hemi, and the
//!   corresponding L1StandardBridgeProxy on Ethereum. Event signatures
//!   confirmed from the Hemi explorer.
//!
//! - **BTC routes** — A separate Hemi-specific tunnel contract. The event
//!   shapes here are derived from Hemi's architecture; the contract address
//!   must still be confirmed from Hemi documentation.
//!   FIXME: confirm BTC tunnel contract address with Hemi docs.
//!
//! - **BitcoinKitV1** — A read-only precompile on Hemi that exposes Bitcoin
//!   chain state. Has NO events — only view functions. Confirmed from Hemi
//!   mainnet explorer. Used by the Bitcoin watcher to verify deposits and
//!   read OP_RETURN data without relying solely on a Bitcoin RPC node.

use alloy::primitives::{Address, U256};
use alloy::sol;
use strait_core::error::{Result, StraitError};

// ============================================================================
// OP Stack Standard Bridge (ETH / ERC-20 routes)
// Confirmed from Hemi mainnet explorer — L2StandardBridge implementation at
// 0xC0d3c0d3c0D3c0d3C0D3c0D3C0d3C0D3C0D30010
// ============================================================================

sol! {
    /// OP Stack L2StandardBridge — handles ETH and ERC-20 tunnel transfers.
    /// Deployed at 0x4200000000000000000000000000000000000010 on Hemi (and
    /// Hemi Sepolia). The proxy resolves to the implementation above.
    interface IStandardBridge {

        // ETH deposits: Ethereum → Hemi
        event ETHBridgeFinalized(
            address indexed from,
            address indexed to,
            uint256 amount,
            bytes extraData
        );

        // ETH withdrawals initiated: Hemi → Ethereum
        event ETHBridgeInitiated(
            address indexed from,
            address indexed to,
            uint256 amount,
            bytes extraData
        );

        // ERC-20 deposits: Ethereum → Hemi
        event ERC20BridgeFinalized(
            address indexed localToken,
            address indexed remoteToken,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );

        // ERC-20 withdrawals initiated: Hemi → Ethereum
        event ERC20BridgeInitiated(
            address indexed localToken,
            address indexed remoteToken,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );

        // Legacy deposit event (older OP Stack versions, still emitted)
        event DepositFinalized(
            address indexed l1Token,
            address indexed l2Token,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );

        // Legacy withdrawal event
        event WithdrawalInitiated(
            address indexed l1Token,
            address indexed l2Token,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );
    }
}

// ============================================================================
// Bitcoin Tunnel Contract (BTC routes)
// FIXME: Contract address not yet confirmed — confirm with Hemi docs/explorer.
// Event shapes reflect Hemi's architecture for Bitcoin-native tunneling.
// ============================================================================

sol! {
    /// Hemi Bitcoin tunnel contract — handles BTC ↔ hBTC transfers.
    interface IBitcoinTunnel {

        /// Emitted when a Bitcoin deposit is claimed on Hemi (BTC → Hemi).
        /// The `txid` field is the Bitcoin deposit txid and is the primary
        /// join key used by the join engine to correlate across chains.
        event TunnelIn(
            address indexed receiver,
            bytes sender,
            uint256 amount,
            bytes32 txid,
            uint256 blockHeight,
            uint32 vout
        );

        /// Emitted when a withdrawal to Bitcoin is initiated (Hemi → BTC).
        event TunnelOut(
            address indexed sender,
            bytes receiver,
            uint256 amount,
            uint256 nonce
        );

        /// Emitted when the Bitcoin withdrawal transaction is confirmed.
        event TunnelOutComplete(
            uint256 indexed nonce,
            bytes32 txid
        );

        /// Emitted when a Proof of Publication is submitted on Hemi.
        event PoPSubmitted(
            bytes32 indexed txid,
            uint256 blockHeight,
            bytes32 merkleRoot,
            bytes proof
        );

        /// Check if a Bitcoin txid has already been claimed.
        function claimed(bytes32 txid) external view returns (bool);

        /// Get the vault address holding deposited BTC.
        function vault() external view returns (address);
    }
}

// ============================================================================
// BitcoinKitV1 precompile interface
// Confirmed from Hemi mainnet explorer (verified, Solidity 0.8.26).
// Mainnet:  0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12
// Testnet:  0xeC9fa5daC1118963933e1A675a4EEA0009b7f215  (v0 — slightly different API)
//
// This contract has NO events — it is a pure read precompile that exposes
// Bitcoin chain state directly from Hemi's embedded Bitcoin node.
// Use it to:
//   - Verify a Bitcoin txid exists and is confirmed
//   - Read OP_RETURN outputs (Output.isOpReturn / Output.opReturnData) to
//     extract the Hemi destination address encoded in tunnel deposits
//   - Watch custody addresses for new UTXOs instead of polling Bitcoin RPC
// ============================================================================

sol! {
    /// BitcoinKitV1 — read-only precompile exposing Bitcoin state on Hemi.
    interface IBitcoinKitV1 {

        /// Get all UTXOs for a Bitcoin address (paginated).
        function getUTXOsForBitcoinAddress(
            string calldata btcAddress,
            uint32 pageNumber,
            uint32 pageSize
        ) external view returns (UTXO[] memory);

        /// Get the number of confirmations for a Bitcoin transaction.
        function getTxConfirmations(bytes32 txId) external view returns (uint32 confirmations);

        /// Get the balance of a Bitcoin address in satoshis.
        function getBitcoinAddressBalance(string calldata btcAddress) external view returns (uint64 balance);

        /// Get the locking script for a Bitcoin address.
        function getScriptForAddress(string calldata btcAddress) external view returns (bytes memory script);

        /// Check if a Bitcoin address is valid.
        function isAddressValid(string calldata btcAddress) external view returns (bool);

        /// Check if a Bitcoin transaction exists in the chain.
        function transactionExists(bytes32 txId) external view returns (bool exists);

        /// Get a full transaction by txid.
        /// The Transaction struct contains an outputs array; each Output has
        /// isOpReturn (bool) and opReturnData (bytes) fields.
        function getTransactionByTxId(bytes32 txId) external view returns (Transaction memory);

        /// Get all inputs for a transaction — more efficient than getTransactionByTxId
        /// when only the inputs are needed.
        function getTransactionInputsByTxId(bytes32 txId) external view returns (Input[] memory);

        /// Get all outputs for a transaction — more efficient than getTransactionByTxId
        /// when only the outputs are needed. Use this when scanning for OP_RETURN outputs.
        function getTransactionOutputsByTxId(bytes32 txId) external view returns (Output[] memory);

        /// Get the number of inputs in a transaction.
        function getTxInputCount(bytes32 txId) external view returns (uint32 txInputCount);

        /// Get the number of outputs in a transaction.
        function getTxOutputCount(bytes32 txId) external view returns (uint32 txOutputCount);

        /// Get a specific input of a transaction.
        function getSpecificTxInput(bytes32 txId, uint32 inputIndex) external view returns (Input memory);

        /// Get a specific output of a transaction.
        /// Use this with the known OP_RETURN output index to read opReturnData
        /// without fetching the entire transaction.
        function getSpecificTxOutput(bytes32 txId, uint32 outputIndex) external view returns (Output memory);

        /// Get the most recent Bitcoin block header seen by Hemi.
        function getLastHeader() external view returns (BitcoinHeader memory);

        /// Get the Bitcoin block header at a specific height.
        function getHeaderN(uint32 height) external view returns (BitcoinHeader memory);
    }

    // -------------------------------------------------------------------------
    // Structs (from BitcoinKitStructs.sol, confirmed from source)
    // -------------------------------------------------------------------------

    struct UTXO {
        bytes32 txId;
        uint256 index;
        uint256 value;           // in satoshis
        bytes scriptPubKey;
    }

    struct Transaction {
        bytes32 containingBlockHash;
        uint256 transactionVersion;
        uint256 size;
        uint256 vSize;
        uint256 lockTime;
        Input[] inputs;
        Output[] outputs;
        uint256 totalInputs;
        uint256 totalOutputs;
        bool containsAllInputs;
        bool containsAllOutputs;
    }

    struct Input {
        uint256 witnessElements;
        uint256 inValue;
        bytes32 inputTxId;
        uint256 sourceIndex;
        bytes scriptSig;
        uint256 sequence;
        uint256 fullScriptSigLength;
        bool containsFullScriptSig;
    }

    /// Details of the transaction that spent an output.
    /// Populated only when Output.isSpent == true.
    struct SpentDetail {
        bytes32 spendingTxId; // Transaction that spent this output
        uint256 inputIndex;   // Index of the input in the spending transaction
    }

    struct Output {
        uint256 outValue;        // in satoshis
        bytes script;
        string outputAddress;
        bool isOpReturn;
        bytes opReturnData;      // raw OP_RETURN payload — contains the Hemi
                                 // destination address for tunnel deposits
                                 // FIXME: confirm exact byte encoding with Hemi docs
        bool isSpent;
        uint256 fullScriptLength;
        bool containsFullScript;
        SpentDetail spentDetail; // Populated when isSpent == true
    }

    struct BitcoinHeader {
        uint32 height;
        bytes32 blockHash;
        uint32 version;
        bytes32 previousBlockHash;
        bytes32 merkleRoot;
        uint32 timestamp;
        uint32 bits;
        uint32 nonce;
    }
}

// ============================================================================
// Event decoder structs
// ============================================================================

/// Decoded ETHBridgeFinalized event (ETH deposit arriving on Hemi).
#[derive(Debug, Clone)]
pub struct EthBridgeFinalizedEvent {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded ETHBridgeInitiated event (ETH withdrawal leaving Hemi).
#[derive(Debug, Clone)]
pub struct EthBridgeInitiatedEvent {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded ERC20BridgeFinalized event (ERC-20 deposit arriving on Hemi).
#[derive(Debug, Clone)]
pub struct Erc20BridgeFinalizedEvent {
    pub local_token: Address,
    pub remote_token: Address,
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded ERC20BridgeInitiated event (ERC-20 withdrawal leaving Hemi).
#[derive(Debug, Clone)]
pub struct Erc20BridgeInitiatedEvent {
    pub local_token: Address,
    pub remote_token: Address,
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded TunnelIn event (BTC deposit claimed on Hemi).
#[derive(Debug, Clone)]
pub struct TunnelInEvent {
    pub receiver: Address,
    pub sender_bytes: Vec<u8>,
    pub amount: U256,
    pub txid: [u8; 32],
    pub block_height: U256,
    pub vout: u32,
}

/// Decoded TunnelOut event (BTC withdrawal initiated from Hemi).
#[derive(Debug, Clone)]
pub struct TunnelOutEvent {
    pub sender: Address,
    pub receiver_bytes: Vec<u8>,
    pub amount: U256,
    pub nonce: U256,
}

/// Decoded TunnelOutComplete event (BTC withdrawal confirmed on Bitcoin).
#[derive(Debug, Clone)]
pub struct TunnelOutCompleteEvent {
    pub nonce: U256,
    pub txid: [u8; 32],
}

/// Decoded PoPSubmitted event.
#[derive(Debug, Clone)]
pub struct PoPSubmittedEvent {
    pub txid: [u8; 32],
    pub block_height: U256,
    pub merkle_root: [u8; 32],
    pub proof: Vec<u8>,
}

// ============================================================================
// Topic selectors
// ============================================================================

/// Event topic selectors for log filtering.
pub mod topics {
    use alloy::primitives::{keccak256, B256};

    // Standard bridge events (ETH / ERC-20 routes)
    pub fn eth_bridge_finalized() -> B256 {
        keccak256(b"ETHBridgeFinalized(address,address,uint256,bytes)")
    }
    pub fn eth_bridge_initiated() -> B256 {
        keccak256(b"ETHBridgeInitiated(address,address,uint256,bytes)")
    }
    pub fn erc20_bridge_finalized() -> B256 {
        keccak256(b"ERC20BridgeFinalized(address,address,address,address,uint256,bytes)")
    }
    pub fn erc20_bridge_initiated() -> B256 {
        keccak256(b"ERC20BridgeInitiated(address,address,address,address,uint256,bytes)")
    }
    pub fn deposit_finalized() -> B256 {
        keccak256(b"DepositFinalized(address,address,address,address,uint256,bytes)")
    }
    pub fn withdrawal_initiated() -> B256 {
        keccak256(b"WithdrawalInitiated(address,address,address,address,uint256,bytes)")
    }

    // BTC tunnel events
    pub fn tunnel_in() -> B256 {
        keccak256(b"TunnelIn(address,bytes,uint256,bytes32,uint256,uint32)")
    }
    pub fn tunnel_out() -> B256 {
        keccak256(b"TunnelOut(address,bytes,uint256,uint256)")
    }
    pub fn tunnel_out_complete() -> B256 {
        keccak256(b"TunnelOutComplete(uint256,bytes32)")
    }
    pub fn pop_submitted() -> B256 {
        keccak256(b"PoPSubmitted(bytes32,uint256,bytes32,bytes)")
    }
}

// ============================================================================
// Contract addresses
// ============================================================================

pub mod addresses {
    use alloy::primitives::Address;

    // ---- L2StandardBridge on Hemi (mainnet + Sepolia) ----
    // Confirmed: well-known OP Stack address, verified on Hemi explorer.

    /// L2StandardBridge on Hemi mainnet and Hemi Sepolia.
    pub const HEMI_L2_STANDARD_BRIDGE: Address =
        alloy::primitives::address!("4200000000000000000000000000000000000010");

    // ---- L1StandardBridgeProxy on Ethereum ----
    // Confirmed from the Hemi contract addresses doc.

    /// L1StandardBridgeProxy on Ethereum mainnet.
    pub const ETH_L1_STANDARD_BRIDGE: Address =
        alloy::primitives::address!("5eaa10F99e7e6D177eF9F74E519E319aa49f191e");

    /// L1StandardBridgeProxy on Sepolia (Hemi testnet counterpart).
    pub const ETH_SEPOLIA_L1_STANDARD_BRIDGE: Address =
        alloy::primitives::address!("c94b1BEe63A3e101FE5F71C80F912b4F4b055925");

    // ---- BitcoinKit precompile ----
    // Confirmed from Hemi explorer (verified source).

    /// BitcoinKit v1 on Hemi mainnet.
    pub const HEMI_BITCOIN_KIT_V1: Address =
        alloy::primitives::address!("7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12");

    /// BitcoinKit v0 on Hemi Sepolia (testnet).
    pub const HEMI_SEPOLIA_BITCOIN_KIT_V0: Address =
        alloy::primitives::address!("eC9fa5daC1118963933e1A675a4EEA0009b7f215");

    // ---- Bitcoin tunnel contract ----
    // FIXME: address not yet confirmed — confirm with Hemi docs/explorer.

    /// Hemi Bitcoin tunnel contract (mainnet). Address TBC.
    pub const HEMI_BITCOIN_TUNNEL: Address =
        alloy::primitives::address!("0000000000000000000000000000000000000000");

    /// Hemi Bitcoin tunnel contract (testnet). Address TBC.
    pub const HEMI_SEPOLIA_BITCOIN_TUNNEL: Address =
        alloy::primitives::address!("0000000000000000000000000000000000000000");
}

// ============================================================================
// Utilities
// ============================================================================

pub fn bytes32_to_txid(bytes: &[u8; 32]) -> String {
    hex::encode(bytes)
}

pub fn txid_to_bytes32(txid: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(txid)
        .map_err(|e| StraitError::Parse(format!("Invalid txid hex: {}", e)))?;
    if bytes.len() != 32 {
        return Err(StraitError::Parse(format!(
            "Txid must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut result = [0u8; 32];
    result.copy_from_slice(&bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_selectors_are_unique() {
        let selectors = [
            topics::eth_bridge_finalized(),
            topics::eth_bridge_initiated(),
            topics::erc20_bridge_finalized(),
            topics::erc20_bridge_initiated(),
            topics::tunnel_in(),
            topics::tunnel_out(),
            topics::tunnel_out_complete(),
            topics::pop_submitted(),
        ];
        for i in 0..selectors.len() {
            for j in (i + 1)..selectors.len() {
                assert_ne!(selectors[i], selectors[j], "Duplicate topic at indices {i} and {j}");
            }
        }
    }

    #[test]
    fn test_topic_selectors_are_deterministic() {
        assert_eq!(topics::eth_bridge_finalized(), topics::eth_bridge_finalized());
        assert_eq!(topics::tunnel_in(), topics::tunnel_in());
    }

    #[test]
    fn test_txid_conversion_roundtrip() {
        let txid_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bytes = txid_to_bytes32(txid_hex).unwrap();
        assert_eq!(bytes32_to_txid(&bytes), txid_hex);
    }

    #[test]
    fn test_invalid_txid_rejected() {
        assert!(txid_to_bytes32("invalid").is_err());
        assert!(txid_to_bytes32("0123456789abcdef").is_err()); // too short
    }
}
