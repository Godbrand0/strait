//! Event storage and queries

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};

use strait_core::error::{Result, StraitError};
use strait_core::types::Chain;

use crate::db::Database;

/// Indexed event record
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct IndexedEvent {
    pub id: i64,
    pub chain: String,
    pub block_height: i64,
    pub block_hash: String,
    pub tx_hash: String,
    pub log_index: Option<i32>,
    pub event_type: String,
    pub contract_address: Option<String>,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Parameters for creating a new event
#[derive(Debug, Clone)]
pub struct CreateEvent {
    pub chain: Chain,
    pub block_height: i64,
    pub block_hash: String,
    pub tx_hash: String,
    pub log_index: Option<i32>,
    pub event_type: String,
    pub contract_address: Option<String>,
    pub data: serde_json::Value,
}

/// Event repository
pub struct EventRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> EventRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    /// Create a new event
    pub async fn create(&self, params: CreateEvent) -> Result<IndexedEvent> {
        let chain_name = chain_to_string(params.chain);
        
        let event = sqlx::query_as::<_, IndexedEvent>(
            r#"
            INSERT INTO events (
                chain, block_height, block_hash, tx_hash, log_index,
                event_type, contract_address, data
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#
        )
        .bind(chain_name)
        .bind(params.block_height)
        .bind(&params.block_hash)
        .bind(&params.tx_hash)
        .bind(params.log_index)
        .bind(&params.event_type)
        .bind(&params.contract_address)
        .bind(&params.data)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(event)
    }

    /// Create multiple events in a batch
    pub async fn create_batch(&self, events: Vec<CreateEvent>) -> Result<u64> {
        if events.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await
            .map_err(|e| StraitError::Database(e))?;

        let mut count = 0u64;
        for params in events {
            let chain_name = chain_to_string(params.chain);
            
            sqlx::query(
                r#"
                INSERT INTO events (
                    chain, block_height, block_hash, tx_hash, log_index,
                    event_type, contract_address, data
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#
            )
            .bind(chain_name)
            .bind(params.block_height)
            .bind(&params.block_hash)
            .bind(&params.tx_hash)
            .bind(params.log_index)
            .bind(&params.event_type)
            .bind(&params.contract_address)
            .bind(&params.data)
            .execute(&mut *tx)
            .await
            .map_err(|e| StraitError::Database(e))?;

            count += 1;
        }

        tx.commit().await
            .map_err(|e| StraitError::Database(e))?;

        Ok(count)
    }

    /// Get event by ID
    pub async fn get_by_id(&self, id: i64) -> Result<Option<IndexedEvent>> {
        let event = sqlx::query_as::<_, IndexedEvent>(
            "SELECT * FROM events WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(event)
    }

    /// Get events by transaction hash
    pub async fn get_by_tx_hash(&self, tx_hash: &str) -> Result<Vec<IndexedEvent>> {
        let events = sqlx::query_as::<_, IndexedEvent>(
            "SELECT * FROM events WHERE tx_hash = $1 ORDER BY log_index"
        )
        .bind(tx_hash)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(events)
    }

    /// Get events by chain and block range
    pub async fn get_by_block_range(
        &self,
        chain: Chain,
        from_height: i64,
        to_height: i64,
    ) -> Result<Vec<IndexedEvent>> {
        let chain_name = chain_to_string(chain);
        
        let events = sqlx::query_as::<_, IndexedEvent>(
            r#"
            SELECT * FROM events
            WHERE chain = $1
              AND block_height >= $2
              AND block_height <= $3
            ORDER BY block_height, log_index
            "#
        )
        .bind(chain_name)
        .bind(from_height)
        .bind(to_height)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(events)
    }

    /// Get events by type
    pub async fn get_by_type(
        &self,
        chain: Chain,
        event_type: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IndexedEvent>> {
        let chain_name = chain_to_string(chain);
        
        let events = sqlx::query_as::<_, IndexedEvent>(
            r#"
            SELECT * FROM events
            WHERE chain = $1 AND event_type = $2
            ORDER BY block_height DESC, log_index
            LIMIT $3 OFFSET $4
            "#
        )
        .bind(chain_name)
        .bind(event_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(events)
    }

    /// Get events by contract address
    pub async fn get_by_contract(
        &self,
        chain: Chain,
        contract_address: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IndexedEvent>> {
        let chain_name = chain_to_string(chain);
        
        let events = sqlx::query_as::<_, IndexedEvent>(
            r#"
            SELECT * FROM events
            WHERE chain = $1 AND contract_address = $2
            ORDER BY block_height DESC, log_index
            LIMIT $3 OFFSET $4
            "#
        )
        .bind(chain_name)
        .bind(contract_address)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(events)
    }

    /// Get latest block height for a chain
    pub async fn get_latest_block(&self, chain: Chain) -> Result<Option<i64>> {
        let chain_name = chain_to_string(chain);
        
        let result = sqlx::query_as::<_, (Option<i64>,)>(
            "SELECT MAX(block_height) FROM events WHERE chain = $1"
        )
        .bind(chain_name)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(result.0)
    }

    /// Delete events older than specified days
    pub async fn delete_old(&self, older_than_days: i64) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM events
            WHERE created_at < NOW() - INTERVAL '1 day' * $1
            "#
        )
        .bind(older_than_days)
        .execute(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(result.rows_affected())
    }

    /// Count events by chain
    pub async fn count_by_chain(&self, chain: Chain) -> Result<i64> {
        let chain_name = chain_to_string(chain);
        
        let count = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM events WHERE chain = $1"
        )
        .bind(chain_name)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(count.0)
    }
}

/// Convert Chain enum to database string
fn chain_to_string(chain: Chain) -> &'static str {
    match chain {
        Chain::Bitcoin => "bitcoin",
        Chain::Ethereum => "ethereum",
        Chain::Hemi => "hemi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_to_string() {
        assert_eq!(chain_to_string(Chain::Bitcoin), "bitcoin");
        assert_eq!(chain_to_string(Chain::Ethereum), "ethereum");
        assert_eq!(chain_to_string(Chain::Hemi), "hemi");
    }
}