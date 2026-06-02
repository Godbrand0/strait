//! Transfer storage and queries

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use strait_core::error::{Result, StraitError};

use crate::db::Database;

/// Transfer status enum matching database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "transfer_status", rename_all = "snake_case")]
pub enum TransferStatus {
    Pending,
    BitcoinSent,
    BitcoinConfirmed,
    EvmClaimed,
    Completed,
    Failed,
}

impl std::fmt::Display for TransferStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::BitcoinSent => write!(f, "bitcoin_sent"),
            Self::BitcoinConfirmed => write!(f, "bitcoin_confirmed"),
            Self::EvmClaimed => write!(f, "evm_claimed"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Tunnel direction enum matching database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "tunnel_direction", rename_all = "snake_case")]
pub enum TunnelDirection {
    BitcoinToEvm,
    EvmToBitcoin,
}

impl std::fmt::Display for TunnelDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BitcoinToEvm => write!(f, "bitcoin_to_evm"),
            Self::EvmToBitcoin => write!(f, "evm_to_bitcoin"),
        }
    }
}

/// Transfer record from database
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Transfer {
    pub id: Uuid,
    pub direction: TunnelDirection,
    pub status: TransferStatus,
    
    // Bitcoin side
    pub bitcoin_txid: Option<String>,
    pub bitcoin_block: Option<i64>,
    pub bitcoin_vout: Option<i32>,
    pub bitcoin_amount: Option<sqlx::types::BigDecimal>,
    
    // EVM side
    pub evm_tx_hash: Option<String>,
    pub evm_block: Option<i64>,
    pub evm_log_index: Option<i32>,
    pub evm_amount: Option<sqlx::types::BigDecimal>,
    
    // Tunnel metadata
    pub sender_address: String,
    pub receiver_address: String,
    pub tunnel_id: Option<String>,
    
    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
}

/// Parameters for creating a new transfer
#[derive(Debug, Clone)]
pub struct CreateTransfer {
    pub direction: TunnelDirection,
    pub sender_address: String,
    pub receiver_address: String,
    pub tunnel_id: Option<String>,
    pub bitcoin_txid: Option<String>,
    pub bitcoin_block: Option<i64>,
    pub bitcoin_vout: Option<i32>,
    pub bitcoin_amount: Option<u64>,
    pub evm_tx_hash: Option<String>,
    pub evm_block: Option<i64>,
    pub evm_log_index: Option<i32>,
    pub evm_amount: Option<String>,
}

/// Transfer repository
pub struct TransferRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> TransferRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    /// Create a new transfer
    pub async fn create(&self, params: CreateTransfer) -> Result<Transfer> {
        let transfer = sqlx::query_as::<_, Transfer>(
            r#"
            INSERT INTO transfers (
                direction, sender_address, receiver_address, tunnel_id,
                bitcoin_txid, bitcoin_block, bitcoin_vout, bitcoin_amount,
                evm_tx_hash, evm_block, evm_log_index, evm_amount
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#
        )
        .bind(params.direction)
        .bind(&params.sender_address)
        .bind(&params.receiver_address)
        .bind(&params.tunnel_id)
        .bind(&params.bitcoin_txid)
        .bind(params.bitcoin_block)
        .bind(params.bitcoin_vout)
        .bind(params.bitcoin_amount.map(|v| sqlx::types::BigDecimal::from(v)))
        .bind(&params.evm_tx_hash)
        .bind(params.evm_block)
        .bind(params.evm_log_index)
        .bind(params.evm_amount.and_then(|v| v.parse::<sqlx::types::BigDecimal>().ok()))
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// Get transfer by ID
    pub async fn get_by_id(&self, id: Uuid) -> Result<Option<Transfer>> {
        let transfer = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// Get transfer by Bitcoin txid
    pub async fn get_by_bitcoin_txid(&self, txid: &str) -> Result<Option<Transfer>> {
        let transfer = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE bitcoin_txid = $1"
        )
        .bind(txid)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// Get transfer by EVM tx hash
    pub async fn get_by_evm_tx_hash(&self, tx_hash: &str) -> Result<Option<Transfer>> {
        let transfer = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE evm_tx_hash = $1"
        )
        .bind(tx_hash)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// Update transfer status
    pub async fn update_status(
        &self,
        id: Uuid,
        status: TransferStatus,
    ) -> Result<Transfer> {
        let transfer = sqlx::query_as::<_, Transfer>(
            r#"
            UPDATE transfers
            SET status = $1,
                confirmed_at = CASE WHEN $1 = 'bitcoin_confirmed' THEN NOW() ELSE confirmed_at END,
                claimed_at = CASE WHEN $1 = 'evm_claimed' THEN NOW() ELSE claimed_at END
            WHERE id = $2
            RETURNING *
            "#
        )
        .bind(status)
        .bind(id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// Update Bitcoin details
    pub async fn update_bitcoin_details(
        &self,
        id: Uuid,
        txid: &str,
        block_height: i64,
        vout: i32,
        amount: u64,
    ) -> Result<Transfer> {
        let transfer = sqlx::query_as::<_, Transfer>(
            r#"
            UPDATE transfers
            SET bitcoin_txid = $1,
                bitcoin_block = $2,
                bitcoin_vout = $3,
                bitcoin_amount = $4,
                status = CASE WHEN status = 'pending' THEN 'bitcoin_sent' ELSE status END
            WHERE id = $5
            RETURNING *
            "#
        )
        .bind(txid)
        .bind(block_height)
        .bind(vout)
        .bind(sqlx::types::BigDecimal::from(amount))
        .bind(id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// Update EVM details
    pub async fn update_evm_details(
        &self,
        id: Uuid,
        tx_hash: &str,
        block_height: i64,
        log_index: i32,
        amount: &str,
    ) -> Result<Transfer> {
        let evm_amount = amount.parse::<sqlx::types::BigDecimal>()
            .map_err(|e| StraitError::Parse(format!("Invalid amount: {}", e)))?;

        let transfer = sqlx::query_as::<_, Transfer>(
            r#"
            UPDATE transfers
            SET evm_tx_hash = $1,
                evm_block = $2,
                evm_log_index = $3,
                evm_amount = $4
            WHERE id = $5
            RETURNING *
            "#
        )
        .bind(tx_hash)
        .bind(block_height)
        .bind(log_index)
        .bind(evm_amount)
        .bind(id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfer)
    }

    /// List transfers with pagination
    pub async fn list(
        &self,
        status: Option<TransferStatus>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Transfer>> {
        let transfers = if let Some(status) = status {
            sqlx::query_as::<_, Transfer>(
                "SELECT * FROM transfers WHERE status = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
            )
            .bind(status)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
        } else {
            sqlx::query_as::<_, Transfer>(
                "SELECT * FROM transfers ORDER BY created_at DESC LIMIT $1 OFFSET $2"
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
        }
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfers)
    }

    /// Get transfers by sender address
    pub async fn list_by_sender(
        &self,
        sender: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE sender_address = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        )
        .bind(sender)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfers)
    }

    /// Get transfers by receiver address
    pub async fn list_by_receiver(
        &self,
        receiver: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE receiver_address = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        )
        .bind(receiver)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfers)
    }

    /// Get pending transfers older than specified duration
    pub async fn get_stale_transfers(&self, older_than_minutes: i64) -> Result<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            r#"
            SELECT * FROM transfers
            WHERE status = 'pending'
              AND created_at < NOW() - INTERVAL '1 minute' * $1
            ORDER BY created_at ASC
            "#
        )
        .bind(older_than_minutes)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(transfers)
    }

    /// Delete old completed transfers (for cleanup)
    pub async fn delete_old_completed(&self, older_than_days: i64) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM transfers
            WHERE status IN ('completed', 'failed')
              AND updated_at < NOW() - INTERVAL '1 day' * $1
            "#
        )
        .bind(older_than_days)
        .execute(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_status_display() {
        assert_eq!(TransferStatus::Pending.to_string(), "pending");
        assert_eq!(TransferStatus::BitcoinSent.to_string(), "bitcoin_sent");
        assert_eq!(TransferStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn test_tunnel_direction_display() {
        assert_eq!(TunnelDirection::BitcoinToEvm.to_string(), "bitcoin_to_evm");
        assert_eq!(TunnelDirection::EvmToBitcoin.to_string(), "evm_to_bitcoin");
    }
}