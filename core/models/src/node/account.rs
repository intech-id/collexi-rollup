use crate::params;
use crate::primitives::GetBits;

use std::collections::BTreeSet;
use std::convert::TryInto;

use crypto_exports::franklin_crypto::bellman::pairing::ff::{self, PrimeField};
use crypto_exports::franklin_crypto::eddsa::PublicKey;
use failure::ensure;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Engine;
use super::Fr;
use super::{AccountId, AccountUpdates, Nonce, TokenId};
use crate::circuit::account::{CircuitAccount, Token};
use crate::circuit::utils::{eth_address_to_fr, pub_key_hash_bytes};
use crate::merkle_tree::rescue_hasher::BabyRescueHasher;
use crate::node::{public_key_from_private, PrivateKey};
use web3::types::Address;

#[derive(Clone, PartialEq, Default, Eq, Hash, PartialOrd, Ord)]
pub struct PubKeyHash {
    pub data: [u8; params::FR_ADDRESS_LEN],
}

impl std::fmt::Debug for PubKeyHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl PubKeyHash {
    pub fn zero() -> Self {
        PubKeyHash {
            data: [0; params::FR_ADDRESS_LEN],
        }
    }

    pub fn to_hex(&self) -> String {
        format!("sync:{}", hex::encode(&self.data))
    }

    pub fn from_hex(s: &str) -> Result<Self, failure::Error> {
        ensure!(s.starts_with("sync:"), "PubKeyHash should start with sync:");
        let bytes = hex::decode(&s[5..])?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(bytes.len() == params::FR_ADDRESS_LEN, "Size mismatch");
        Ok(PubKeyHash {
            data: bytes.try_into().unwrap(),
        })
    }

    pub fn from_pubkey(public_key: &PublicKey<Engine>) -> Self {
        let mut pk_hash =
            pub_key_hash_bytes(public_key, &params::RESCUE_HASHER as &BabyRescueHasher);
        pk_hash.reverse();
        Self::from_bytes(&pk_hash).expect("pk convert error")
    }

    pub fn to_fr(&self) -> Fr {
        ff::from_hex(&format!("0x{}", hex::encode(&self.data))).unwrap()
    }

    pub fn from_privkey(private_key: &PrivateKey) -> Self {
        let pub_key = public_key_from_private(&private_key);
        Self::from_pubkey(&pub_key)
    }
}

impl Serialize for PubKeyHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for PubKeyHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        String::deserialize(deserializer).and_then(|string| {
            PubKeyHash::from_hex(&string).map_err(|err| Error::custom(err.to_string()))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub pub_key_hash: PubKeyHash,
    pub address: Address,
    tokens: BTreeSet<TokenId>,
    pub nonce: Nonce,
}

impl PartialEq for Account {
    fn eq(&self, other: &Account) -> bool {
        self.get_bits_le().eq(&other.get_bits_le())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountUpdate {
    Create {
        address: Address,
        nonce: Nonce,
    },
    Delete {
        address: Address,
        nonce: Nonce,
    },
    AddToken {
        old_nonce: Nonce,
        new_nonce: Nonce,
        token: TokenId,
    },
    RemoveToken {
        old_nonce: Nonce,
        new_nonce: Nonce,
        token: TokenId,
    },
    ChangePubKeyHash {
        old_pub_key_hash: PubKeyHash,
        new_pub_key_hash: PubKeyHash,
        old_nonce: Nonce,
        new_nonce: Nonce,
    },
}

// TODO: Check if coding to Fr is the same as in the circuit.
impl From<Account> for CircuitAccount<super::Engine> {
    fn from(acc: Account) -> Self {
        let mut circuit_account = CircuitAccount::default();

        let tokens: Vec<_> = acc
            .tokens
            .iter()
            .enumerate()
            .map(|(index, token)| {
                (
                    *token as u32,
                    Token {
                        id: Fr::from_str(&token.to_string()).unwrap(),
                    },
                )
            })
            .collect();

        for (i, b) in tokens.into_iter() {
            circuit_account.subtree.insert(u32::from(i), b);
        }

        circuit_account.nonce = Fr::from_str(&acc.nonce.to_string()).unwrap();
        circuit_account.pub_key_hash = acc.pub_key_hash.to_fr();
        circuit_account.address = eth_address_to_fr(&acc.address);
        circuit_account
    }
}

impl AccountUpdate {
    pub fn reversed_update(&self) -> Self {
        match self {
            AccountUpdate::Create { address, nonce } => AccountUpdate::Delete {
                address: *address,
                nonce: *nonce,
            },
            AccountUpdate::Delete { address, nonce } => AccountUpdate::Create {
                address: *address,
                nonce: *nonce,
            },
            AccountUpdate::AddToken {
                old_nonce,
                new_nonce,
                token,
            } => AccountUpdate::RemoveToken {
                old_nonce: *new_nonce,
                new_nonce: *old_nonce,
                token: *token,
            },
            AccountUpdate::RemoveToken {
                old_nonce,
                new_nonce,
                token,
            } => AccountUpdate::AddToken {
                old_nonce: *new_nonce,
                new_nonce: *old_nonce,
                token: *token,
            },
            AccountUpdate::ChangePubKeyHash {
                old_pub_key_hash,
                new_pub_key_hash,
                old_nonce,
                new_nonce,
            } => AccountUpdate::ChangePubKeyHash {
                old_pub_key_hash: new_pub_key_hash.clone(),
                new_pub_key_hash: old_pub_key_hash.clone(),
                old_nonce: *new_nonce,
                new_nonce: *old_nonce,
            },
        }
    }
}

impl Default for Account {
    fn default() -> Self {
        Self {
            tokens: BTreeSet::new(),
            nonce: 0,
            pub_key_hash: PubKeyHash::default(),
            address: Address::zero(),
        }
    }
}

impl GetBits for Account {
    fn get_bits_le(&self) -> Vec<bool> {
        CircuitAccount::<super::Engine>::from(self.clone()).get_bits_le()
    }
}

impl Account {
    pub fn default_with_address(address: &Address) -> Account {
        let mut account = Account::default();
        account.address = *address;
        account
    }

    pub fn create_account(id: AccountId, address: Address) -> (Account, AccountUpdates) {
        let mut account = Account::default();
        account.address = address;
        let updates = vec![(
            id,
            AccountUpdate::Create {
                address: account.address,
                nonce: account.nonce,
            },
        )];
        (account, updates)
    }

    pub fn has_token(&self, token_id: TokenId) -> bool {
        self.tokens.contains(&token_id)
    }

    pub fn add_token(&mut self, token_id: TokenId) {
        self.tokens.insert(token_id);
    }

    pub fn remove_token(&mut self, token_id: TokenId) {
        self.tokens.remove(&token_id);
    }

    pub fn get_tokens(&self) -> Vec<TokenId> {
        self.tokens.clone().into_iter().collect()
    }

    pub fn apply_updates(mut account: Option<Self>, updates: &[AccountUpdate]) -> Option<Self> {
        for update in updates {
            account = Account::apply_update(account, update.clone());
        }
        account
    }

    pub fn apply_update(account: Option<Self>, update: AccountUpdate) -> Option<Self> {
        match account {
            Some(mut account) => match update {
                AccountUpdate::Delete { .. } => None,
                AccountUpdate::AddToken {
                    new_nonce, token, ..
                } => {
                    account.add_token(token);
                    account.nonce = new_nonce;
                    Some(account)
                }
                AccountUpdate::RemoveToken {
                    new_nonce, token, ..
                } => {
                    account.remove_token(token);
                    account.nonce = new_nonce;
                    Some(account)
                }
                AccountUpdate::ChangePubKeyHash {
                    new_pub_key_hash,
                    new_nonce,
                    ..
                } => {
                    account.pub_key_hash = new_pub_key_hash;
                    account.nonce = new_nonce;
                    Some(account)
                }
                _ => {
                    error!(
                        "Incorrect update received {:?} for account {:?}",
                        update, account
                    );
                    Some(account)
                }
            },
            None => match update {
                AccountUpdate::Create { address, nonce, .. } => {
                    let mut new_account = Account::default();
                    new_account.address = address;
                    new_account.nonce = nonce;
                    Some(new_account)
                }
                _ => {
                    error!("Incorrect update received {:?} for empty account", update);
                    None
                }
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::node::{apply_updates, reverse_updates, AccountMap, AccountUpdates};

    #[test]
    fn test_default_account() {
        let a = Account::default();
        a.get_bits_le();
    }

    #[test]
    fn test_account_update() {
        let create = AccountUpdate::Create {
            address: Address::default(),
            nonce: 1,
        };

        let add_token = AccountUpdate::AddToken {
            old_nonce: 1,
            new_nonce: 2,
            token: 1,
        };

        let remove_token = AccountUpdate::RemoveToken {
            old_nonce: 1,
            new_nonce: 2,
            token: 1,
        };

        let delete = AccountUpdate::Delete {
            address: Address::default(),
            nonce: 2,
        };

        {
            {
                let mut created_account = Account::default();
                created_account.nonce = 1;
                assert_eq!(
                    Account::apply_update(None, create.clone())
                        .unwrap()
                        .get_bits_le(),
                    created_account.get_bits_le()
                );
            }

            assert!(Account::apply_update(None, add_token.clone()).is_none());
            assert!(Account::apply_update(None, delete.clone()).is_none());
        }
        {
            assert_eq!(
                Account::apply_update(Some(Account::default()), create)
                    .unwrap()
                    .get_bits_le(),
                Account::default().get_bits_le()
            );
            {
                let mut updated_account = Account::default();
                updated_account.nonce = 2;
                updated_account.add_token(1);
                assert_eq!(
                    Account::apply_update(Some(Account::default()), add_token)
                        .unwrap()
                        .get_bits_le(),
                    updated_account.get_bits_le()
                );
            }
            assert!(Account::apply_update(Some(Account::default()), delete).is_none());
        }
        {
            let mut initial_account = Account::default();
            initial_account.add_token(1);
            let mut updated_account = Account::default();
            updated_account.nonce = 2;
            assert_eq!(
                Account::apply_update(Some(initial_account), remove_token)
                    .unwrap()
                    .get_bits_le(),
                updated_account.get_bits_le()
            );
        }
    }

    #[test]
    fn test_account_updates() {
        // Create two accounts: 0, 1
        // In updates -> delete 0, update balance of 1, create account 2
        // Reverse updates

        let account_map_initial = {
            let mut map = AccountMap::default();
            let mut account_0 = Account::default();
            account_0.nonce = 8;
            let mut account_1 = Account::default();
            account_1.nonce = 16;
            map.insert(0, account_0);
            map.insert(1, account_1);
            map
        };

        let account_map_updated_expected = {
            let mut map = AccountMap::default();
            let mut account_1 = Account::default();
            account_1.nonce = 17;
            account_1.add_token(1);
            map.insert(1, account_1);
            let mut account_2 = Account::default();
            account_2.nonce = 36;
            map.insert(2, account_2);
            map
        };

        let updates = {
            let mut updates = AccountUpdates::new();
            updates.push((
                0,
                AccountUpdate::Delete {
                    address: Address::default(),
                    nonce: 8,
                },
            ));
            updates.push((
                1,
                AccountUpdate::AddToken {
                    old_nonce: 16,
                    new_nonce: 17,
                    token: 1,
                },
            ));
            updates.push((
                2,
                AccountUpdate::Create {
                    address: Address::default(),
                    nonce: 36,
                },
            ));
            updates
        };

        let account_map_updated = {
            let mut map = account_map_initial.clone();
            apply_updates(&mut map, updates.clone());
            map
        };

        assert_eq!(account_map_updated, account_map_updated_expected);

        let account_map_updated_back = {
            let mut map = account_map_updated;
            let mut reversed = updates;
            reverse_updates(&mut reversed);
            apply_updates(&mut map, reversed);
            map
        };

        assert_eq!(account_map_updated_back, account_map_initial);
    }
}
