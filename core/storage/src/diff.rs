// Built-in deps
use std::cmp::Ordering;
// External imports
use web3::types::Address;
// Workspace imports
use models::node::PubKeyHash;
use models::node::{AccountUpdate, TokenId};
// Local imports
use crate::chain::account::records::*;

/// `StorageAccoundDiff` is a enum that combines all the possible
/// changes that can be applied to account, which includes:
///
/// - Creation of the new account.
/// - Removing of the existing account.
/// - Changing balance of the account.
/// - Changing the public key of the account.
///
/// This enum allows one to process account updates in a generic way.
#[derive(Debug)]
pub enum StorageAccountDiff {
    AddToken(StorageAccountUpdate),
    RemoveToken(StorageAccountUpdate),
    Create(StorageAccountCreation),
    Delete(StorageAccountCreation),
    ChangePubKey(StorageAccountPubkeyUpdate),
}

impl From<StorageAccountUpdate> for StorageAccountDiff {
    fn from(update: StorageAccountUpdate) -> Self {
        if update.added {
            StorageAccountDiff::AddToken(update)
        } else {
            StorageAccountDiff::RemoveToken(update)
        }
    }
}

impl From<StorageAccountCreation> for StorageAccountDiff {
    fn from(create: StorageAccountCreation) -> Self {
        if create.is_create {
            StorageAccountDiff::Create(create)
        } else {
            StorageAccountDiff::Delete(create)
        }
    }
}

impl From<StorageAccountPubkeyUpdate> for StorageAccountDiff {
    fn from(update: StorageAccountPubkeyUpdate) -> Self {
        StorageAccountDiff::ChangePubKey(update)
    }
}

impl Into<(u32, AccountUpdate)> for StorageAccountDiff {
    fn into(self) -> (u32, AccountUpdate) {
        match self {
            StorageAccountDiff::AddToken(upd) => (
                upd.account_id as u32,
                AccountUpdate::AddToken {
                    old_nonce: upd.old_nonce as u32,
                    new_nonce: upd.new_nonce as u32,
                    token: upd.token_id as u16, // TODO ADE: type conversion
                },
            ),
            StorageAccountDiff::RemoveToken(upd) => (
                upd.account_id as u32,
                AccountUpdate::RemoveToken {
                    old_nonce: upd.old_nonce as u32,
                    new_nonce: upd.new_nonce as u32,
                    token: upd.token_id as u16, // TODO ADE: type conversion
                },
            ),
            StorageAccountDiff::Create(upd) => (
                upd.account_id as u32,
                AccountUpdate::Create {
                    nonce: upd.nonce as u32,
                    address: Address::from_slice(&upd.address.as_slice()),
                },
            ),
            StorageAccountDiff::Delete(upd) => (
                upd.account_id as u32,
                AccountUpdate::Delete {
                    nonce: upd.nonce as u32,
                    address: Address::from_slice(&upd.address.as_slice()),
                },
            ),
            StorageAccountDiff::ChangePubKey(upd) => (
                upd.account_id as u32,
                AccountUpdate::ChangePubKeyHash {
                    old_nonce: upd.old_nonce as u32,
                    new_nonce: upd.new_nonce as u32,
                    old_pub_key_hash: PubKeyHash::from_bytes(&upd.old_pubkey_hash)
                        .expect("PubkeyHash update from db deserialize"),
                    new_pub_key_hash: PubKeyHash::from_bytes(&upd.new_pubkey_hash)
                        .expect("PubkeyHash update from db deserialize"),
                },
            ),
        }
    }
}

impl StorageAccountDiff {
    /// Returns the index of the operation within block.
    pub fn update_order_id(&self) -> i32 {
        *match self {
            StorageAccountDiff::AddToken(StorageAccountUpdate {
                update_order_id, ..
            }) => update_order_id,
            StorageAccountDiff::RemoveToken(StorageAccountUpdate {
                update_order_id, ..
            }) => update_order_id,
            StorageAccountDiff::Create(StorageAccountCreation {
                update_order_id, ..
            }) => update_order_id,
            StorageAccountDiff::Delete(StorageAccountCreation {
                update_order_id, ..
            }) => update_order_id,
            StorageAccountDiff::ChangePubKey(StorageAccountPubkeyUpdate {
                update_order_id,
                ..
            }) => update_order_id,
        }
    }

    /// Compares updates by `block number` then by `update_order_id` (which is number within block).
    pub fn cmp_order(&self, other: &Self) -> Ordering {
        self.block_number()
            .cmp(&other.block_number())
            .then(self.update_order_id().cmp(&other.update_order_id()))
    }

    /// Returns the block index to which the operation belongs.
    pub fn block_number(&self) -> i64 {
        *match self {
            StorageAccountDiff::AddToken(StorageAccountUpdate { block_number, .. }) => block_number,
            StorageAccountDiff::RemoveToken(StorageAccountUpdate { block_number, .. }) => {
                block_number
            }
            StorageAccountDiff::Create(StorageAccountCreation { block_number, .. }) => block_number,
            StorageAccountDiff::Delete(StorageAccountCreation { block_number, .. }) => block_number,
            StorageAccountDiff::ChangePubKey(StorageAccountPubkeyUpdate {
                block_number, ..
            }) => block_number,
        }
    }
}
