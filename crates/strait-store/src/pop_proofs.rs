//! POP (Proof of Payment) storage and queries

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use strait_core::error::{Result, StraitError};

use crate::db::Database;

/// POP proof status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "pop_proof_status", rename_all = "snake_case")]
pub enum PopProofStatus {
    Pending,
    Verified,
    Claimed,
    Failed,
}

impl std::fmt::Display for PopProofStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Verified => write!(f, "verified"),
            Self::Claimed => write!(f, "claimed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// POP proof record from database
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PopProof {
    pub id: i64,
    pub status: PopProofStatus,
    
    // Bitcoin transaction details
    pub bitcoin_txid: String,
    pub bitcoin_block_height: i64,
    pub bitcoin_block_hash: String,
    pub bitcoin_vout: i32,
    pub bitcoin_amount: sqlx::types::BigDecimal,
    
    // POP proof data
    pub merkle_root: String,
    pub merkle_proof: serde_json::Value,
    pub block_header: String,
    
    // Recipient on EVM
    pub recipient_address: String,
    pub tunnel_id: Option<String>,
    
    // Claim details
    pub claimed_tx_hash: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    
    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Parameters for creating a new POP proof
#[derive(Debug, Clone)]
pub struct CreatePopProof {
    pub bitcoin_txid: String,
    pub bitcoin_block_height: i64,
    pub bitcoin_block_hash: String,
    pub bitcoin_vout: i32,
    pub bitcoin_amount: u64,
    pub merkle_root: String,
    pub merkle_proof: serde_json::Value,
    pub block_header: String,
    pub recipient_address: String,
    pub tunnel_id: Option<String>,
}

/// POP proof repository
pub struct PopProofRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> PopProofRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    /// Create a new POP proof
    pub async fn create(&self, params: CreatePopProof) -> Result<PopProof> {
        let proof = sqlx::query_as::<_, PopProof>(
            r#"
            INSERT INTO pop_proofs (
                bitcoin_txid, bitcoin_block_height, bitcoin_block_hash,
                bitcoin_vout, bitcoin_amount, merkle_root, merkle_proof,
                block_header, recipient_address, tunnel_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
            "#
        )
        .bind(&params.bitcoin_txid)
        .bind(params.bitcoin_block_height)
        .bind(&params.bitcoin_block_hash)
        .bind(params.bitcoin_vout)
        .bind(sqlx::types::BigDecimal::from(params.bitcoin_amount))
        .bind(&params.merkle_root)
        .bind(&params.merkle_proof)
        .bind(&params.block_header)
        .bind(&params.recipient_address)
        .bind(&params.tunnel_id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proof)
    }

    /// Get POP proof by ID
    pub async fn get_by_id(&self, id: i64) -> Result<Option<PopProof>> {
        let proof = sqlx::query_as::<_, PopProof>(
            "SELECT * FROM pop_proofs WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proof)
    }

    /// Get POP proof by Bitcoin txid
    pub async fn get_by_bitcoin_txid(&self, txid: &str) -> Result<Option<PopProof>> {
        let proof = sqlx::query_as::<_, PopProof>(
            "SELECT * FROM pop_proofs WHERE bitcoin_txid = $1"
        )
        .bind(txid)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proof)
    }

    /// Get POP proofs by recipient address
    pub async fn get_by_recipient(
        &self,
        recipient: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PopProof>> {
        let proofs = sqlx::query_as::<_, PopProof>(
            r#"
            SELECT * FROM pop_proofs
            WHERE recipient_address = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#
        )
        .bind(recipient)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proofs)
    }

    /// Get pending POP proofs
    pub async fn get_pending(&self, limit: i64) -> Result<Vec<PopProof>> {
        let proofs = sqlx::query_as::<_, PopProof>(
            r#"
            SELECT * FROM pop_proofs
            WHERE status = 'pending'
            ORDER BY created_at ASC
            LIMIT $1
            "#
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proofs)
    }

    /// Update POP proof status
    pub async fn update_status(
        &self,
        id: i64,
        status: PopProofStatus,
    ) -> Result<PopProof> {
        let proof = sqlx::query_as::<_, PopProof>(
            r#"
            UPDATE pop_proofs
            SET status = $1, updated_at = NOW()
            WHERE id = $2
            RETURNING *
            "#
        )
        .bind(status)
        .bind(id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proof)
    }

    /// Mark POP proof as claimed
    pub async fn mark_claimed(
        &self,
        id: i64,
        tx_hash: &str,
    ) -> Result<PopProof> {
        let proof = sqlx::query_as::<_, PopProof>(
            r#"
            UPDATE pop_proofs
            SET status = 'claimed',
                claimed_tx_hash = $1,
                claimed_at = NOW(),
                updated_at = NOW()
            WHERE id = $2
            RETURNING *
            "#
        )
        .bind(tx_hash)
        .bind(id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(proof)
    }

    /// List POP proofs with pagination
    pub async fn list(
        &self,
        status: Option<PopProofStatus>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PopProof>> {
        let proofs = if let Some(status) = status {
            sqlx::query_as::<_, PopProof>(
                r#"
                SELECT * FROM pop_proofs
                WHERE status = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#
            )
            .bind(status)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
        } else {
            sqlx::query_as::<_, PopProof>(
                r#"
                SELECT * FROM pop_proofs
                ORDER BY created_at DESC
                LIMIT $1 OFFSET $2
                "#
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
        }
        .map_err(|e| StraitError::Database(e))?;

        Ok(proofs)
    }

    /// Delete old POP proofs
    pub async fn delete_old(&self, older_than_days: i64) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM pop_proofs
            WHERE status IN ('claimed', 'failed')
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
    fn test_pop_proof_status_display() {
        assert_eq!(PopProofStatus::Pending.to_string(), "pending");
        assert_eq!(PopProofStatus::Verified.to_string(), "verified");
        assert_eq!(PopProofStatus::Claimed.to_string(), "claimed");
        assert_eq!(PopProofStatus::Failed.to_string(), "failed");
    }
}