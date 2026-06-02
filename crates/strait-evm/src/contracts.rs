//! Tunnel contract ABIs and log decoders.
//!
//! Provides ABI definitions and event decoders for Hemi tunnel contracts.

use alloy::primitives::{Address, U256};
use alloy_sol_types::sol;
use strait_core::error::{Result, StraitError};

// ============================================================================
// Hemi Tunnel Contract ABI
// ============================================================================

sol! {
    /// Main tunnel contract interface for Bitcoin ↔ EVM transfers.
    interface ITunnel {
        // -------------------------------------------------------------------
        // Events
        // -------------------------------------------------------------------

        /// Emitted when a Bitcoin deposit is claimed on EVM (Bitcoin → EVM).
        /// @param sender Bitcoin address that sent the deposit
        /// @param receiver EVM address receiving the funds
        /// @param amount Amount in satoshis
        /// @param txid Bitcoin transaction ID
        /// @param blockHeight Bitcoin block height
        /// @param vout Output index in the Bitcoin transaction
        event TunnelIn(
            address indexed receiver,
            bytes sender,
            uint256 amount,
            bytes32 txid,
            uint256 blockHeight,
            uint32 vout
        );

        /// Emitted when a withdrawal is initiated (EVM → Bitcoin).
        /// @param sender EVM address sending the funds
        /// @param receiver Bitcoin address to receive
        /// @param amount Amount in satoshis
        /// @param nonce Unique nonce for the withdrawal
        event TunnelOut(
            address indexed sender,
            bytes receiver,
            uint256 amount,
            uint256 nonce
        );

        /// Emitted when a withdrawal is completed (Bitcoin tx confirmed).
        /// @param nonce Withdrawal nonce
        /// @param txid Bitcoin transaction ID that completed the withdrawal
        event TunnelOutComplete(
            uint256 indexed nonce,
            bytes32 txid
        );

        /// Emitted when a Proof of Publication is submitted.
        /// @param txid Bitcoin transaction ID
        /// @param blockHeight Bitcoin block height
        /// @param merleRoot Merkle root of the block
        /// @param proof Merkle proof
        event PoPSubmitted(
            bytes32 indexed txid,
            uint256 blockHeight,
            bytes32 merkleRoot,
            bytes proof
        );

        /// Emitted when a PoP is verified.
        /// @param txid Bitcoin transaction ID
        /// @param verified Whether the proof was valid
        event PoPVerified(
            bytes32 indexed txid,
            bool verified
        );

        // -------------------------------------------------------------------
        // View Functions
        // -------------------------------------------------------------------

        /// Get the tunnel fee in basis points.
        function feeBps() external view returns (uint256);

        /// Get the minimum deposit amount in satoshis.
        function minDeposit() external view returns (uint256);

        /// Get the maximum deposit amount in satoshis.
        function maxDeposit() external view returns (uint256);

        /// Get the withdrawal nonce for an address.
        function nonces(address account) external view returns (uint256);

        /// Check if a Bitcoin txid has been claimed.
        function claimed(bytes32 txid) external view returns (bool);

        /// Get the vault address (where deposits are held).
        function vault() external view returns (address);

        /// Get the PoP verifier contract address.
        function popVerifier() external view returns (address);

        // -------------------------------------------------------------------
        // State-Changing Functions
        // -------------------------------------------------------------------

        /// Claim a Bitcoin deposit (Bitcoin → EVM).
        /// @param txid Bitcoin transaction ID (32 bytes)
        /// @param blockHeight Bitcoin block height
        /// @param vout Output index
        /// @param amount Amount in satoshis
        /// @param sender Bitcoin sender address (bytes)
        /// @param proof Merkle proof of the transaction
        function claim(
            bytes32 txid,
            uint256 blockHeight,
            uint32 vout,
            uint256 amount,
            bytes calldata sender,
            bytes calldata proof
        ) external;

        /// Initiate a withdrawal (EVM → Bitcoin).
        /// @param receiver Bitcoin address to receive funds
        /// @param amount Amount in satoshis
        function withdraw(bytes calldata receiver, uint256 amount) external;

        /// Submit a Proof of Publication.
        /// @param txid Bitcoin transaction ID
        /// @param blockHeight Bitcoin block height
        /// @param merkleRoot Merkle root of the block
        /// @param proof Merkle proof
        function submitPoP(
            bytes32 txid,
            uint256 blockHeight,
            bytes32 merkleRoot,
            bytes calldata proof
        ) external;
    }
}

// ============================================================================
// PoP (Proof of Publication) Verifier Contract
// ============================================================================

sol! {
    /// Proof of Publication verifier contract.
    interface IPoPVerifier {
        /// Emitted when a new block header is registered.
        event BlockHeaderRegistered(
            uint256 indexed blockHeight,
            bytes32 merkleRoot,
            bytes32 blockHash
        );

        /// Register a Bitcoin block header.
        /// @param blockHeight Block height
        /// @param header Block header (80 bytes)
        function registerBlockHeader(uint256 blockHeight, bytes calldata header) external;

        /// Verify a transaction is in a block.
        /// @param txid Transaction ID
        /// @param blockHeight Block height
        /// @param merkleRoot Merkle root
        /// @param proof Merkle proof
        function verify(
            bytes32 txid,
            uint256 blockHeight,
            bytes32 merkleRoot,
            bytes calldata proof
        ) external view returns (bool);

        /// Get the merkle root for a block height.
        function merkleRoots(uint256 blockHeight) external view returns (bytes32);
    }
}

// ============================================================================
// Event Decoders
// ============================================================================

/// Decoded TunnelIn event data.
#[derive(Debug, Clone)]
pub struct TunnelInEvent {
    pub receiver: Address,
    pub sender: Vec<u8>,
    pub amount: U256,
    pub txid: [u8; 32],
    pub block_height: U256,
    pub vout: u32,
}

/// Decoded TunnelOut event data.
#[derive(Debug, Clone)]
pub struct TunnelOutEvent {
    pub sender: Address,
    pub receiver: Vec<u8>,
    pub amount: U256,
    pub nonce: U256,
}

/// Decoded TunnelOutComplete event data.
#[derive(Debug, Clone)]
pub struct TunnelOutCompleteEvent {
    pub nonce: U256,
    pub txid: [u8; 32],
}

/// Decoded PoPSubmitted event data.
#[derive(Debug, Clone)]
pub struct PoPSubmittedEvent {
    pub txid: [u8; 32],
    pub block_height: U256,
    pub merkle_root: [u8; 32],
    pub proof: Vec<u8>,
}

/// Decoded PoPVerified event data.
#[derive(Debug, Clone)]
pub struct PoPVerifiedEvent {
    pub txid: [u8; 32],
    pub verified: bool,
}

/// Event topic selectors for filtering logs.
pub mod topics {
    use alloy::primitives::{keccak256, B256};

    /// TunnelIn(address,bytes,uint256,bytes32,uint256,uint32)
    pub fn tunnel_in() -> B256 {
        keccak256(b"TunnelIn(address,bytes,uint256,bytes32,uint256,uint32)")
    }

    /// TunnelOut(address,bytes,uint256,uint256)
    pub fn tunnel_out() -> B256 {
        keccak256(b"TunnelOut(address,bytes,uint256,uint256)")
    }

    /// TunnelOutComplete(uint256,bytes32)
    pub fn tunnel_out_complete() -> B256 {
        keccak256(b"TunnelOutComplete(uint256,bytes32)")
    }

    /// PoPSubmitted(bytes32,uint256,bytes32,bytes)
    pub fn pop_submitted() -> B256 {
        keccak256(b"PoPSubmitted(bytes32,uint256,bytes32,bytes)")
    }

    /// PoPVerified(bytes32,bool)
    pub fn pop_verified() -> B256 {
        keccak256(b"PoPVerified(bytes32,bool)")
    }
}

/// Contract addresses for different networks.
pub mod addresses {
    use alloy::primitives::Address;

    /// Hemi mainnet tunnel contract address.
    /// TODO: Update with actual deployed address.
    pub const HEMI_TUNNEL: Address = alloy::primitives::address!(
        "0x0000000000000000000000000000000000000000"
    );

    /// Hemi testnet tunnel contract address.
    /// TODO: Update with actual deployed address.
    pub const HEMI_TESTNET_TUNNEL: Address = alloy::primitives::address!(
        "0x0000000000000000000000000000000000000000"
    );

    /// Ethereum mainnet tunnel contract address (if applicable).
    pub const ETHEREUM_TUNNEL: Address = alloy::primitives::address!(
        "0x0000000000000000000000000000000000000000"
    );
}

/// Helper to convert bytes32 to txid string (hex without 0x prefix).
pub fn bytes32_to_txid(bytes: &[u8; 32]) -> String {
    hex::encode(bytes)
}

/// Helper to convert txid string to bytes32.
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
    fn test_topic_selectors() {
        let tunnel_in = topics::tunnel_in();
        let tunnel_out = topics::tunnel_out();
        let tunnel_out_complete = topics::tunnel_out_complete();
        let pop_submitted = topics::pop_submitted();
        let pop_verified = topics::pop_verified();

        // Topics should be unique
        assert_ne!(tunnel_in, tunnel_out);
        assert_ne!(tunnel_in, tunnel_out_complete);
        assert_ne!(tunnel_out, tunnel_out_complete);
        assert_ne!(pop_submitted, pop_verified);

        // Topics should be deterministic
        assert_eq!(tunnel_in, topics::tunnel_in());
    }

    #[test]
    fn test_txid_conversion() {
        let txid_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bytes = txid_to_bytes32(txid_hex).unwrap();
        let back = bytes32_to_txid(&bytes);
        assert_eq!(txid_hex, back);
    }

    #[test]
    fn test_invalid_txid() {
        assert!(txid_to_bytes32("invalid").is_err());
        assert!(txid_to_bytes32("0123456789abcdef").is_err()); // Too short
    }
}