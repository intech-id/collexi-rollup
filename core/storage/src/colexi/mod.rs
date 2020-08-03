use diesel::prelude::*;
use crate::StorageProcessor;

pub mod records;

pub struct ColexiQueries<'a>(pub &'a StorageProcessor);

impl<'a> ColexiQueries<'a> {
  pub fn get_transfer_history(&self, token_id: u16) -> QueryResult<Vec<records::TransferOperation>> {
    let query = diesel::
      sql_query(
        "SELECT
            tx.tx ->> 'from' AS from,
            tx.tx ->> 'to' AS to,
            tx.tx_hash AS tx_hash,
            tx.created_at as created_at,
            tx.block_number as block_number,
            p.block_number as proof_block_number
        FROM executed_transactions tx
        LEFT OUTER JOIN proofs p ON p.block_number = tx.block_number
        WHERE 
            (tx.tx ->> 'tokenId')::NUMERIC = $1
            AND tx.success = TRUE
        ORDER BY tx.id ASC"
      )
      .bind::<diesel::sql_types::Integer,_>(token_id as i32);
    query.get_results(self.0.conn())
  }

  pub fn get_current_owner(&self, token_id: u16) -> QueryResult<Option<records::Account>> {
    let query = diesel::
      sql_query(
        "SELECT a.address AS address
            FROM tokens t 
            LEFT JOIN accounts a ON a.id = t.account_id
            WHERE token_id = $1
            LIMIT 1"
      )
      .bind::<diesel::sql_types::Integer,_>(token_id as i32);
    query.get_result(self.0.conn()).optional()
  }
}