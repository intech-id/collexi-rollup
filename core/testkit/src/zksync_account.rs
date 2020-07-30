// Built-in imports
use std::{fmt, sync::Mutex};
// External uses
use bigdecimal::BigDecimal;
use web3::types::H256;
// Workspace uses
use crypto_exports::rand::{thread_rng, Rng};
use models::node::tx::{ChangePubKey, PackedEthSignature, TxSignature};
use models::node::{
    priv_key_from_fs, AccountId, Address, Close, Nonce, PrivateKey, PubKeyHash, TokenId, Transfer,
    Withdraw,
};

/// Structure used to sign ZKSync transactions, keeps tracks of its nonce internally
pub struct ZksyncAccount {
    pub private_key: PrivateKey,
    pub pubkey_hash: PubKeyHash,
    pub address: Address,
    pub eth_private_key: H256,
    account_id: Mutex<Option<AccountId>>,
    nonce: Mutex<Nonce>,
}

impl fmt::Debug for ZksyncAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // It is OK to disclose the private key contents for a testkit account.
        let mut pk_contents = Vec::new();
        self.private_key
            .write(&mut pk_contents)
            .expect("Failed writing the private key contents");

        f.debug_struct("ZksyncAccount")
            .field("private_key", &pk_contents)
            .field("pubkey_hash", &self.pubkey_hash)
            .field("address", &self.address)
            .field("eth_private_key", &self.eth_private_key)
            .field("nonce", &self.nonce)
            .finish()
    }
}

impl ZksyncAccount {
    /// Note: probably not secure, use for testing.
    pub fn rand() -> Self {
        let rng = &mut thread_rng();

        let pk = priv_key_from_fs(rng.gen());
        let (eth_pk, eth_address) = {
            let eth_pk = rng.gen::<[u8; 32]>().into();
            let eth_address;
            loop {
                if let Ok(address) = PackedEthSignature::address_from_private_key(&eth_pk) {
                    eth_address = address;
                    break;
                }
            }
            (eth_pk, eth_address)
        };
        Self::new(pk, 0, eth_address, eth_pk)
    }

    pub fn new(
        private_key: PrivateKey,
        nonce: Nonce,
        address: Address,
        eth_private_key: H256,
    ) -> Self {
        let pubkey_hash = PubKeyHash::from_privkey(&private_key);
        assert_eq!(
            address,
            PackedEthSignature::address_from_private_key(&eth_private_key)
                .expect("private key is incorrect"),
            "address should correspond to private key"
        );
        Self {
            account_id: Mutex::new(None),
            address,
            private_key,
            pubkey_hash,
            eth_private_key,
            nonce: Mutex::new(nonce),
        }
    }

    pub fn nonce(&self) -> Nonce {
        let n = self.nonce.lock().unwrap();
        *n
    }

    pub fn set_nonce(&self, new_nonce: Nonce) {
        *self.nonce.lock().unwrap() = new_nonce;
    }

    pub fn set_account_id(&self, account_id: Option<AccountId>) {
        *self.account_id.lock().unwrap() = account_id;
    }

    pub fn get_account_id(&self) -> Option<AccountId> {
        *self.account_id.lock().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sign_transfer(
        &self,
        token_id: TokenId,
        token_symbol: &str,
        amount: BigDecimal,
        fee: BigDecimal,
        to: &Address,
        nonce: Option<Nonce>,
        increment_nonce: bool,
    ) -> (Transfer, PackedEthSignature) {
        let mut stored_nonce = self.nonce.lock().unwrap();
        let transfer = Transfer::new_signed(
            self.account_id
                .lock()
                .unwrap()
                .expect("can't sign tx withoud account id"),
            self.address,
            *to,
            token_id,
            amount,
            fee,
            nonce.unwrap_or_else(|| *stored_nonce),
            &self.private_key,
        )
        .expect("Failed to sign transfer");

        if increment_nonce {
            *stored_nonce += 1;
        }

        let eth_signature = PackedEthSignature::sign(
            &self.eth_private_key,
            transfer.get_ethereum_sign_message(token_symbol).as_bytes(),
        )
        .expect("Signing the transfer unexpectedly failed");
        (transfer, eth_signature)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sign_withdraw(
        &self,
        token_id: TokenId,
        token_symbol: &str,
        amount: BigDecimal,
        fee: BigDecimal,
        eth_address: &Address,
        nonce: Option<Nonce>,
        increment_nonce: bool,
    ) -> (Withdraw, PackedEthSignature) {
        let mut stored_nonce = self.nonce.lock().unwrap();
        let withdraw = Withdraw::new_signed(
            self.account_id
                .lock()
                .unwrap()
                .expect("can't sign tx withoud account id"),
            self.address,
            *eth_address,
            token_id,
            amount,
            fee,
            nonce.unwrap_or_else(|| *stored_nonce),
            &self.private_key,
        )
        .expect("Failed to sign withdraw");

        if increment_nonce {
            *stored_nonce += 1;
        }

        let eth_signature = PackedEthSignature::sign(
            &self.eth_private_key,
            withdraw.get_ethereum_sign_message(token_symbol).as_bytes(),
        )
        .expect("Signing the withdraw unexpectedly failed");
        (withdraw, eth_signature)
    }

    pub fn sign_close(&self, nonce: Option<Nonce>, increment_nonce: bool) -> Close {
        let mut stored_nonce = self.nonce.lock().unwrap();
        let mut close = Close {
            account: self.address,
            nonce: nonce.unwrap_or_else(|| *stored_nonce),
            signature: TxSignature::default(),
        };
        close.signature = TxSignature::sign_musig(&self.private_key, &close.get_bytes());

        if increment_nonce {
            *stored_nonce += 1;
        }
        close
    }

    pub fn create_change_pubkey_tx(
        &self,
        nonce: Option<Nonce>,
        increment_nonce: bool,
        auth_onchain: bool,
    ) -> ChangePubKey {
        let account_id = self
            .account_id
            .lock()
            .unwrap()
            .expect("can't sign tx withoud account id");
        let mut stored_nonce = self.nonce.lock().unwrap();
        let nonce = nonce.unwrap_or_else(|| *stored_nonce);
        let eth_signature = if auth_onchain {
            None
        } else {
            let sign_bytes =
                ChangePubKey::get_eth_signed_data(account_id, nonce, &self.pubkey_hash)
                    .expect("Failed to construct change pubkey signed message.");
            let eth_signature = PackedEthSignature::sign(&self.eth_private_key, &sign_bytes)
                .expect("Signature should succeed");
            Some(eth_signature)
        };
        let change_pubkey = ChangePubKey {
            account_id,
            account: self.address,
            new_pk_hash: self.pubkey_hash.clone(),
            nonce,
            eth_signature,
        };

        if !auth_onchain {
            assert!(
                change_pubkey.verify_eth_signature() == Some(self.address),
                "eth signature is incorrect"
            );
        }

        if increment_nonce {
            *stored_nonce += 1;
        }

        change_pubkey
    }
}
