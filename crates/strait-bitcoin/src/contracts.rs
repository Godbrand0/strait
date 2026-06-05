//! BitcoinKit precompile interface and addresses for use within the Bitcoin crate.

use alloy::sol;

// ============================================================================
// BitcoinKit precompile interface (confirmed from Hemi mainnet explorer)
// ============================================================================

sol! {
    struct UTXO {
        bytes32 txId;
        uint256 index;
        uint256 value;
        bytes scriptPubKey;
    }

    /// Details of the transaction that spent an output.
    /// Populated only when Output.isSpent == true.
    struct SpentDetail {
        bytes32 spendingTxId; // Transaction that spent this output
        uint256 inputIndex;   // Index of the input in the spending transaction
    }

    struct Output {
        uint256 outValue;
        bytes script;
        string outputAddress;
        bool isOpReturn;
        bytes opReturnData;   // Raw OP_RETURN payload — Strait reads this to extract
                              // the Hemi destination address from tunnel deposits.
                              // FIXME: confirm exact byte encoding with Hemi docs.
        bool isSpent;
        uint256 fullScriptLength;
        bool containsFullScript;
        SpentDetail spentDetail; // Populated when isSpent == true
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

    interface IBitcoinKitV1 {
        function getUTXOsForBitcoinAddress(
            string calldata btcAddress,
            uint32 pageNumber,
            uint32 pageSize
        ) external view returns (UTXO[] memory);

        function getTxConfirmations(bytes32 txId) external view returns (uint32 confirmations);

        function getBitcoinAddressBalance(string calldata btcAddress) external view returns (uint64 balance);

        function isAddressValid(string calldata btcAddress) external view returns (bool);

        /// Convert a Bitcoin address to its locking script (precompile 0x46).
        /// Supports P2PKH, P2SH, P2WPKH, P2WSH, P2TR.
        /// Useful for verifying custody address scripts without local derivation.
        function getScriptForAddress(string calldata btcAddress) external view returns (bytes memory script);

        function transactionExists(bytes32 txId) external view returns (bool exists);

        function getTransactionByTxId(bytes32 txId) external view returns (Transaction memory);

        function getTransactionInputsByTxId(bytes32 txId) external view returns (Input[] memory);

        function getTransactionOutputsByTxId(bytes32 txId) external view returns (Output[] memory);

        function getTxOutputCount(bytes32 txId) external view returns (uint32 txOutputCount);

        function getSpecificTxOutput(bytes32 txId, uint32 outputIndex) external view returns (Output memory);

        function getLastHeader() external view returns (BitcoinHeader memory);

        function getHeaderN(uint32 height) external view returns (BitcoinHeader memory);
    }
}

// ============================================================================
// Contract addresses
// ============================================================================

pub mod addresses {
    use alloy::primitives::Address;

    /// BitcoinKit v1 on Hemi mainnet (confirmed from explorer).
    pub const HEMI_BITCOIN_KIT_V1: Address =
        alloy::primitives::address!("7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12");

    /// BitcoinKit v0 on Hemi Sepolia (confirmed from explorer).
    pub const HEMI_SEPOLIA_BITCOIN_KIT_V0: Address =
        alloy::primitives::address!("eC9fa5daC1118963933e1A675a4EEA0009b7f215");
}
