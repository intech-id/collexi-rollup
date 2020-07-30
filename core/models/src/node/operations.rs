use super::tx::TxSignature;
use super::AccountId;
use super::FranklinTx;
use crate::node::tx::ChangePubKey;
use crate::node::{
    pack_fee_amount, pack_token_amount, unpack_fee_amount, unpack_token_amount, Close, Deposit,
    FranklinPriorityOp, FullExit, PubKeyHash, Transfer, Withdraw,
};
use crate::params::{
    ACCOUNT_ID_BIT_WIDTH, ADDRESS_WIDTH, ETH_ADDRESS_BIT_WIDTH, FEE_EXPONENT_BIT_WIDTH,
    FEE_MANTISSA_BIT_WIDTH, FR_ADDRESS_LEN, NEW_PUBKEY_HASH_WIDTH, NONCE_BIT_WIDTH,
    TOKENID_BIT_WIDTH,
};
use crate::primitives::{
    big_decimal_to_u128, bytes_slice_to_uint128, bytes_slice_to_uint16, bytes_slice_to_uint32,
    u128_to_bigdecimal,
};
use bigdecimal::BigDecimal;
use failure::{ensure, format_err};
use web3::types::Address;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositOp {
    pub priority_op: Deposit,
    pub account_id: AccountId,
}

impl DepositOp {
    pub const CHUNKS: usize = 6;
    pub const OP_CODE: u8 = 0x01;

    pub fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        data.extend_from_slice(&self.priority_op.token_id.to_be_bytes());
        data.extend_from_slice(&self.priority_op.to.as_bytes());
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes.len() == Self::CHUNKS * 8,
            "Wrong bytes length for deposit pubdata"
        );

        let account_id_offset = 1;
        let token_id_offset = account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let account_address_offset = token_id_offset + TOKENID_BIT_WIDTH / 8;

        let account_id = bytes_slice_to_uint32(
            &bytes[account_id_offset..account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8],
        )
        .ok_or_else(|| format_err!("Cant get account id from deposit pubdata"))?;
        let token_id =
            bytes_slice_to_uint16(&bytes[token_id_offset..token_id_offset + TOKENID_BIT_WIDTH / 8])
                .ok_or_else(|| format_err!("Cant get token_id from deposit pubdata"))?;
        let to = Address::from_slice(
            &bytes[account_address_offset..account_address_offset + FR_ADDRESS_LEN],
        );

        let from = Address::default(); // unknown from pubdata.

        Ok(Self {
            priority_op: Deposit { from, token_id, to },
            account_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoopOp {}

impl NoopOp {
    pub const CHUNKS: usize = 1;
    pub const OP_CODE: u8 = 0x00;

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes == [0, 0, 0, 0, 0, 0, 0, 0],
            format!("Wrong pubdata for noop operation {:?}", bytes)
        );
        Ok(Self {})
    }

    fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferToNewOp {
    pub tx: Transfer,
    pub from: AccountId,
    pub to: AccountId,
}

impl TransferToNewOp {
    pub const CHUNKS: usize = 5;
    pub const OP_CODE: u8 = 0x02;

    fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.from.to_be_bytes()[1..]);
        data.extend_from_slice(&self.tx.token_id.to_be_bytes());
        data.extend_from_slice(&self.tx.to.as_bytes());
        data.extend_from_slice(&self.to.to_be_bytes()[1..]);
        //data.extend_from_slice(&pack_fee_amount(&self.tx.fee));
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes.len() == Self::CHUNKS * 8,
            "Wrong bytes length for transfer to new pubdata"
        );

        let from_offset = 1;
        let token_id_offset = from_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let to_address_offset = token_id_offset + TOKENID_BIT_WIDTH / 8;
        let to_id_offset = to_address_offset + FR_ADDRESS_LEN;
        let fee_offset = to_id_offset + ACCOUNT_ID_BIT_WIDTH / 8;

        let from_id =
            bytes_slice_to_uint32(&bytes[from_offset..from_offset + ACCOUNT_ID_BIT_WIDTH / 8])
                .ok_or_else(|| {
                    format_err!("Cant get from account id from transfer to new pubdata")
                })?;
        let to_id =
            bytes_slice_to_uint32(&bytes[to_id_offset..to_id_offset + ACCOUNT_ID_BIT_WIDTH / 8])
                .ok_or_else(|| {
                    format_err!("Cant get to account id from transfer to new pubdata")
                })?;
        let from = Address::zero(); // It is unknown from pubdata;
        let to = Address::from_slice(&bytes[to_address_offset..to_address_offset + FR_ADDRESS_LEN]);
        let token_id =
            bytes_slice_to_uint16(&bytes[token_id_offset..token_id_offset + TOKENID_BIT_WIDTH / 8])
                .ok_or_else(|| format_err!("Cant get token id from transfer to new pubdata"))?;
        let fee = unpack_fee_amount(
            &bytes[fee_offset..fee_offset + (FEE_EXPONENT_BIT_WIDTH + FEE_MANTISSA_BIT_WIDTH) / 8],
        )
        .ok_or_else(|| format_err!("Cant get fee from transfer to new pubdata"))?;
        let nonce = 0; // It is unknown from pubdata

        Ok(Self {
            tx: Transfer::new(from_id, from, to, token_id, fee, nonce, None),
            from: from_id,
            to: to_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferOp {
    pub tx: Transfer,
    pub from: AccountId,
    pub to: AccountId,
}

impl TransferOp {
    pub const CHUNKS: usize = 2;
    pub const OP_CODE: u8 = 0x05;

    fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.from.to_be_bytes()[1..]);
        data.extend_from_slice(&self.tx.token_id.to_be_bytes());
        data.extend_from_slice(&self.to.to_be_bytes()[1..]);
        data.extend_from_slice(&pack_fee_amount(&self.tx.fee));
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes.len() == Self::CHUNKS * 8,
            "Wrong bytes length for transfer pubdata"
        );

        let from_offset = 1;
        let token_id_offset = from_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let to_offset = token_id_offset + TOKENID_BIT_WIDTH / 8;
        let fee_offset = to_offset + ACCOUNT_ID_BIT_WIDTH / 8;

        let from_address = Address::zero(); // From pubdata its unknown
        let to_address = Address::zero(); // From pubdata its unknown
        let token_id =
            bytes_slice_to_uint16(&bytes[token_id_offset..token_id_offset + TOKENID_BIT_WIDTH / 8])
                .ok_or_else(|| format_err!("Cant get token id from transfer pubdata"))?;
        let fee = unpack_fee_amount(
            &bytes[fee_offset..fee_offset + (FEE_EXPONENT_BIT_WIDTH + FEE_MANTISSA_BIT_WIDTH) / 8],
        )
        .ok_or_else(|| format_err!("Cant get fee from transfer pubdata"))?;
        let nonce = 0; // It is unknown from pubdata
        let from_id =
            bytes_slice_to_uint32(&bytes[from_offset..from_offset + ACCOUNT_ID_BIT_WIDTH / 8])
                .ok_or_else(|| format_err!("Cant get from account id from transfer pubdata"))?;
        let to_id = bytes_slice_to_uint32(&bytes[to_offset..to_offset + ACCOUNT_ID_BIT_WIDTH / 8])
            .ok_or_else(|| format_err!("Cant get to account id from transfer pubdata"))?;

        Ok(Self {
            tx: Transfer::new(
                from_id,
                from_address,
                to_address,
                token_id,
                fee,
                nonce,
                None,
            ),
            from: from_id,
            to: to_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawOp {
    pub tx: Withdraw,
    pub account_id: AccountId,
}

impl WithdrawOp {
    pub const CHUNKS: usize = 6;
    pub const OP_CODE: u8 = 0x03;
    pub const WITHDRAW_DATA_PREFIX: [u8; 1] = [1];

    fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        data.extend_from_slice(&self.tx.token_id.to_be_bytes());
        data.extend_from_slice(&pack_fee_amount(&self.tx.fee));
        data.extend_from_slice(self.tx.to.as_bytes());
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    fn get_withdrawal_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&Self::WITHDRAW_DATA_PREFIX); // first byte is a bool variable 'addToPendingWithdrawalsQueue'
        data.extend_from_slice(self.tx.to.as_bytes());
        data.extend_from_slice(&self.tx.token_id.to_be_bytes());
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes.len() == Self::CHUNKS * 8,
            "Wrong bytes length for withdraw pubdata"
        );

        let account_offset = 1;
        let token_id_offset = account_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let fee_offset = token_id_offset + TOKENID_BIT_WIDTH / 8;
        let eth_address_offset = fee_offset + (FEE_EXPONENT_BIT_WIDTH + FEE_MANTISSA_BIT_WIDTH) / 8;

        let account_id = bytes_slice_to_uint32(
            &bytes[account_offset..account_offset + ACCOUNT_ID_BIT_WIDTH / 8],
        )
        .ok_or_else(|| format_err!("Cant get account id from withdraw pubdata"))?;
        let from = Address::zero(); // From pubdata it is unknown
        let token_id =
            bytes_slice_to_uint16(&bytes[token_id_offset..token_id_offset + TOKENID_BIT_WIDTH / 8])
                .ok_or_else(|| format_err!("Cant get token id from withdraw pubdata"))?;
        let to = Address::from_slice(
            &bytes[eth_address_offset..eth_address_offset + ETH_ADDRESS_BIT_WIDTH / 8],
        );
        let fee = unpack_fee_amount(
            &bytes[fee_offset..fee_offset + (FEE_EXPONENT_BIT_WIDTH + FEE_MANTISSA_BIT_WIDTH) / 8],
        )
        .ok_or_else(|| format_err!("Cant get fee from withdraw pubdata"))?;
        let nonce = 0; // From pubdata it is unknown

        Ok(Self {
            tx: Withdraw::new(account_id, from, to, token_id, fee, nonce, None),
            account_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseOp {
    pub tx: Close,
    pub account_id: AccountId,
}

impl CloseOp {
    pub const CHUNKS: usize = 1;
    pub const OP_CODE: u8 = 0x04;

    fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes.len() == Self::CHUNKS * 8,
            "Wrong bytes length for close pubdata"
        );

        let account_id_offset = 1;
        let account_id = bytes_slice_to_uint32(
            &bytes[account_id_offset..account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8],
        )
        .ok_or_else(|| format_err!("Cant get from account id from close pubdata"))?;
        let account_address = Address::zero(); // From pubdata it is unknown
        let nonce = 0; // From pubdata it is unknown
        let signature = TxSignature::default(); // From pubdata it is unknown
        Ok(Self {
            tx: Close {
                account: account_address,
                nonce,
                signature,
            },
            account_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePubKeyOp {
    pub tx: ChangePubKey,
    pub account_id: AccountId,
}

impl ChangePubKeyOp {
    pub const CHUNKS: usize = 6;
    pub const OP_CODE: u8 = 0x07;

    pub fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        data.extend_from_slice(&self.tx.new_pk_hash.data);
        data.extend_from_slice(&self.tx.account.as_bytes());
        data.extend_from_slice(&self.tx.nonce.to_be_bytes());
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    pub fn get_eth_witness(&self) -> Vec<u8> {
        if let Some(eth_signature) = &self.tx.eth_signature {
            eth_signature.serialize_packed().to_vec()
        } else {
            Vec::new()
        }
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        let mut offset = 1;

        let mut len = ACCOUNT_ID_BIT_WIDTH / 8;
        ensure!(
            bytes.len() >= offset + len,
            "Change pubkey offchain, pubdata too short"
        );
        let account_id = bytes_slice_to_uint32(&bytes[offset..offset + len])
            .ok_or_else(|| format_err!("Change pubkey offchain, fail to get account id"))?;
        offset += len;

        len = NEW_PUBKEY_HASH_WIDTH / 8;
        ensure!(
            bytes.len() >= offset + len,
            "Change pubkey offchain, pubdata too short"
        );
        let new_pk_hash = PubKeyHash::from_bytes(&bytes[offset..offset + len])?;
        offset += len;

        len = ADDRESS_WIDTH / 8;
        ensure!(
            bytes.len() >= offset + len,
            "Change pubkey offchain, pubdata too short"
        );
        let account = Address::from_slice(&bytes[offset..offset + len]);
        offset += len;

        len = NONCE_BIT_WIDTH / 8;
        ensure!(
            bytes.len() >= offset + len,
            "Change pubkey offchain, pubdata too short"
        );
        let nonce = bytes_slice_to_uint32(&bytes[offset..offset + len])
            .ok_or_else(|| format_err!("Change pubkey offchain, fail to get nonce"))?;

        Ok(ChangePubKeyOp {
            tx: ChangePubKey {
                account_id,
                account,
                new_pk_hash,
                nonce,
                eth_signature: None,
            },
            account_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullExitOp {
    pub priority_op: FullExit,
}

impl FullExitOp {
    pub const CHUNKS: usize = 6;
    pub const OP_CODE: u8 = 0x06;
    pub const WITHDRAW_DATA_PREFIX: [u8; 1] = [0];

    fn get_public_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(Self::OP_CODE); // opcode
        data.extend_from_slice(&self.priority_op.account_id.to_be_bytes()[1..]);
        data.extend_from_slice(self.priority_op.eth_address.as_bytes());
        data.resize(Self::CHUNKS * 8, 0x00);
        data
    }

    fn get_withdrawal_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&Self::WITHDRAW_DATA_PREFIX); // first byte is a bool variable 'addToPendingWithdrawalsQueue'
        data.extend_from_slice(self.priority_op.eth_address.as_bytes());
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(
            bytes.len() == Self::CHUNKS * 8,
            "Wrong bytes length for full exit pubdata"
        );

        let account_id_offset = 1;
        let eth_address_offset = account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let eth_address_endoffset = eth_address_offset + ETH_ADDRESS_BIT_WIDTH / 8;

        let account_id = bytes_slice_to_uint32(&bytes[account_id_offset..eth_address_offset])
            .ok_or_else(|| format_err!("Cant get account id from full exit pubdata"))?;
        let eth_address = Address::from_slice(&bytes[eth_address_offset..eth_address_endoffset]);

        Ok(Self {
            priority_op: FullExit {
                account_id,
                eth_address,
            },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FranklinOp {
    Noop(NoopOp),
    Deposit(Box<DepositOp>),
    TransferToNew(Box<TransferToNewOp>),
    Withdraw(Box<WithdrawOp>),
    Close(Box<CloseOp>),
    Transfer(Box<TransferOp>),
    FullExit(Box<FullExitOp>),
    ChangePubKeyOffchain(Box<ChangePubKeyOp>),
}

impl FranklinOp {
    pub fn chunks(&self) -> usize {
        match self {
            FranklinOp::Noop(_) => NoopOp::CHUNKS,
            FranklinOp::Deposit(_) => DepositOp::CHUNKS,
            FranklinOp::TransferToNew(_) => TransferToNewOp::CHUNKS,
            FranklinOp::Withdraw(_) => WithdrawOp::CHUNKS,
            FranklinOp::Close(_) => CloseOp::CHUNKS,
            FranklinOp::Transfer(_) => TransferOp::CHUNKS,
            FranklinOp::FullExit(_) => FullExitOp::CHUNKS,
            FranklinOp::ChangePubKeyOffchain(_) => ChangePubKeyOp::CHUNKS,
        }
    }

    pub fn public_data(&self) -> Vec<u8> {
        match self {
            FranklinOp::Noop(op) => op.get_public_data(),
            FranklinOp::Deposit(op) => op.get_public_data(),
            FranklinOp::TransferToNew(op) => op.get_public_data(),
            FranklinOp::Withdraw(op) => op.get_public_data(),
            FranklinOp::Close(op) => op.get_public_data(),
            FranklinOp::Transfer(op) => op.get_public_data(),
            FranklinOp::FullExit(op) => op.get_public_data(),
            FranklinOp::ChangePubKeyOffchain(op) => op.get_public_data(),
        }
    }

    pub fn eth_witness(&self) -> Option<Vec<u8>> {
        match self {
            FranklinOp::ChangePubKeyOffchain(op) => Some(op.get_eth_witness()),
            _ => None,
        }
    }

    pub fn withdrawal_data(&self) -> Option<Vec<u8>> {
        match self {
            FranklinOp::Withdraw(op) => Some(op.get_withdrawal_data()),
            FranklinOp::FullExit(op) => Some(op.get_withdrawal_data()),
            _ => None,
        }
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, failure::Error> {
        let op_type: u8 = *bytes.first().ok_or_else(|| format_err!("Empty pubdata"))?;
        match op_type {
            NoopOp::OP_CODE => Ok(FranklinOp::Noop(NoopOp::from_public_data(&bytes)?)),
            DepositOp::OP_CODE => Ok(FranklinOp::Deposit(Box::new(DepositOp::from_public_data(
                &bytes,
            )?))),
            TransferToNewOp::OP_CODE => Ok(FranklinOp::TransferToNew(Box::new(
                TransferToNewOp::from_public_data(&bytes)?,
            ))),
            WithdrawOp::OP_CODE => Ok(FranklinOp::Withdraw(Box::new(
                WithdrawOp::from_public_data(&bytes)?,
            ))),
            CloseOp::OP_CODE => Ok(FranklinOp::Close(Box::new(CloseOp::from_public_data(
                &bytes,
            )?))),
            TransferOp::OP_CODE => Ok(FranklinOp::Transfer(Box::new(
                TransferOp::from_public_data(&bytes)?,
            ))),
            FullExitOp::OP_CODE => Ok(FranklinOp::FullExit(Box::new(
                FullExitOp::from_public_data(&bytes)?,
            ))),
            ChangePubKeyOp::OP_CODE => Ok(FranklinOp::ChangePubKeyOffchain(Box::new(
                ChangePubKeyOp::from_public_data(&bytes)?,
            ))),
            _ => Err(format_err!("Wrong operation type: {}", &op_type)),
        }
    }

    pub fn public_data_length(op_type: u8) -> Result<usize, failure::Error> {
        match op_type {
            NoopOp::OP_CODE => Ok(NoopOp::CHUNKS * 8),
            DepositOp::OP_CODE => Ok(DepositOp::CHUNKS * 8),
            TransferToNewOp::OP_CODE => Ok(TransferToNewOp::CHUNKS * 8),
            WithdrawOp::OP_CODE => Ok(WithdrawOp::CHUNKS * 8),
            CloseOp::OP_CODE => Ok(CloseOp::CHUNKS * 8),
            TransferOp::OP_CODE => Ok(TransferOp::CHUNKS * 8),
            FullExitOp::OP_CODE => Ok(FullExitOp::CHUNKS * 8),
            ChangePubKeyOp::OP_CODE => Ok(ChangePubKeyOp::CHUNKS * 8),
            _ => Err(format_err!("Wrong operation type: {}", &op_type)),
        }
    }

    pub fn try_get_tx(&self) -> Result<FranklinTx, failure::Error> {
        match self {
            FranklinOp::Transfer(op) => Ok(FranklinTx::Transfer(Box::new(op.tx.clone()))),
            FranklinOp::TransferToNew(op) => Ok(FranklinTx::Transfer(Box::new(op.tx.clone()))),
            FranklinOp::Withdraw(op) => Ok(FranklinTx::Withdraw(Box::new(op.tx.clone()))),
            FranklinOp::Close(op) => Ok(FranklinTx::Close(Box::new(op.tx.clone()))),
            FranklinOp::ChangePubKeyOffchain(op) => {
                Ok(FranklinTx::ChangePubKey(Box::new(op.tx.clone())))
            }
            _ => Err(format_err!("Wrong tx type")),
        }
    }

    pub fn try_get_priority_op(&self) -> Result<FranklinPriorityOp, failure::Error> {
        match self {
            FranklinOp::Deposit(op) => Ok(FranklinPriorityOp::Deposit(op.priority_op.clone())),
            FranklinOp::FullExit(op) => Ok(FranklinPriorityOp::FullExit(op.priority_op.clone())),
            _ => Err(format_err!("Wrong operation type")),
        }
    }
}
