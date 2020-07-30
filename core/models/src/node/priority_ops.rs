use super::AccountId;
use super::TokenId;
use crate::params::{
    ACCOUNT_ID_BIT_WIDTH, BALANCE_BIT_WIDTH, ETH_ADDRESS_BIT_WIDTH, FR_ADDRESS_LEN,
    TOKENID_BIT_WIDTH,
};
use crate::primitives::{bytes_slice_to_uint32, u128_to_bigdecimal};
use ethabi::{decode, ParamType};
use failure::{bail, ensure, format_err};
use std::convert::{TryFrom, TryInto};
use web3::types::{Address, Log, U256};

use super::operations::{DepositOp, FullExitOp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deposit {
    pub from: Address,
    pub token_id: TokenId,
    pub to: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullExit {
    pub account_id: AccountId,
    pub eth_address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FranklinPriorityOp {
    Deposit(Deposit),
    FullExit(FullExit),
}

impl FranklinPriorityOp {
    pub fn try_get_deposit(&self) -> Option<Deposit> {
        if let Self::Deposit(deposit) = self {
            Some(deposit.clone())
        } else {
            None
        }
    }

    pub fn parse_from_priority_queue_logs(
        pub_data: &[u8],
        op_type_id: u8,
        sender: Address,
    ) -> Result<Self, failure::Error> {
        // see contracts/contracts/Operations.sol
        match op_type_id {
            DepositOp::OP_CODE => {
                let pub_data_left = pub_data;

                // account_id
                let (_, pub_data_left) = pub_data_left.split_at(ACCOUNT_ID_BIT_WIDTH / 8);

                // token_id
                let (token_id, pub_data_left) = {
                    let (token_id, left) = pub_data_left.split_at(TOKENID_BIT_WIDTH / 8);
                    (u16::from_be_bytes(token_id.try_into().unwrap()), left) // TODO ADE must be encoded as u256!
                };

                // account
                let (account, pub_data_left) = {
                    let (account, left) = pub_data_left.split_at(FR_ADDRESS_LEN);
                    (Address::from_slice(account), left)
                };

                ensure!(
                    pub_data_left.is_empty(),
                    "DepositOp parse failed: input too big"
                );

                Ok(Self::Deposit(Deposit {
                    from: sender,
                    token_id,
                    to: account,
                }))
            }
            FullExitOp::OP_CODE => {
                // account_id
                let (account_id, pub_data_left) = {
                    let (account_id, left) = pub_data.split_at(ACCOUNT_ID_BIT_WIDTH / 8);
                    (bytes_slice_to_uint32(account_id).unwrap(), left)
                };

                // owner
                let (eth_address, pub_data_left) = {
                    let (eth_address, left) = pub_data_left.split_at(ETH_ADDRESS_BIT_WIDTH / 8);
                    (Address::from_slice(eth_address), left)
                };

                // amount
                ensure!(
                    pub_data_left.is_empty(),
                    "FullExitOp parse failed: input too big"
                );

                Ok(Self::FullExit(FullExit {
                    account_id,
                    eth_address,
                }))
            }
            _ => {
                bail!("Unsupported priority op type");
            }
        }
    }

    pub fn chunks(&self) -> usize {
        match self {
            Self::Deposit(_) => DepositOp::CHUNKS,
            Self::FullExit(_) => FullExitOp::CHUNKS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityOp {
    pub serial_id: u64,
    pub data: FranklinPriorityOp,
    pub deadline_block: u64,
    pub eth_hash: Vec<u8>,
}

impl TryFrom<Log> for PriorityOp {
    type Error = failure::Error;

    fn try_from(event: Log) -> Result<PriorityOp, failure::Error> {
        let mut dec_ev = decode(
            &[
                ParamType::Address,
                ParamType::Uint(64),  // Serial id
                ParamType::Uint(8),   // OpType
                ParamType::Bytes,     // Pubdata
                ParamType::Uint(256), // expir. block
            ],
            &event.data.0,
        )
        .map_err(|e| format_err!("Event data decode: {:?}", e))?;

        let sender = dec_ev.remove(0).to_address().unwrap();
        Ok(PriorityOp {
            serial_id: dec_ev
                .remove(0)
                .to_uint()
                .as_ref()
                .map(U256::as_u64)
                .unwrap(),
            data: {
                let op_type = dec_ev
                    .remove(0)
                    .to_uint()
                    .as_ref()
                    .map(|ui| U256::as_u32(ui) as u8)
                    .unwrap();
                let op_pubdata = dec_ev.remove(0).to_bytes().unwrap();
                FranklinPriorityOp::parse_from_priority_queue_logs(&op_pubdata, op_type, sender)
                    .expect("Failed to parse priority op data")
            },
            deadline_block: dec_ev
                .remove(0)
                .to_uint()
                .as_ref()
                .map(U256::as_u64)
                .unwrap(),
            eth_hash: event
                .transaction_hash
                .expect("Event transaction hash is missing")
                .as_bytes()
                .to_vec(),
        })
    }
}
