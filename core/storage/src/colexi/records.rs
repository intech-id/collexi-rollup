use diesel::sql_types::{Nullable, BigInt, Binary, Text, Timestamp};
use chrono::NaiveDateTime;

#[derive(Debug, QueryableByName)]
pub struct TransferOperation {
    #[sql_type = "BigInt"]
    pub block_number: i64,
    #[sql_type = "Text"]
    pub from: String,
    #[sql_type = "Text"]
    pub to: String,
    #[sql_type = "Binary"]
    pub tx_hash: Vec<u8>,
    #[sql_type = "Nullable<BigInt>"]
    pub proof_block_number: Option<i64>,
    #[sql_type = "Timestamp"]
    pub created_at: NaiveDateTime,
}

#[derive(Debug, QueryableByName)]
pub struct Account {
    #[sql_type = "Binary"]
    pub address: Vec<u8>
}