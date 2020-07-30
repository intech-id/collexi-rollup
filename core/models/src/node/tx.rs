use super::{Nonce, TokenId};

use crate::node::{
    is_fee_amount_packable, pack_fee_amount, public_key_from_private, AccountId, CloseOp,
    TransferOp, WithdrawOp,
};
use bigdecimal::BigDecimal;
use crypto::{digest::Digest, sha2::Sha256};

use super::account::PubKeyHash;
use super::Engine;
use crate::franklin_crypto::alt_babyjubjub::fs::FsRepr;
use crate::franklin_crypto::alt_babyjubjub::JubjubEngine;
use crate::franklin_crypto::alt_babyjubjub::{edwards, AltJubjubBn256};
use crate::franklin_crypto::bellman::pairing::ff::{PrimeField, PrimeFieldRepr};
use crate::franklin_crypto::eddsa::{PrivateKey, PublicKey, Seed, Signature};
use crate::franklin_crypto::jubjub::FixedGenerators;
use crate::franklin_crypto::rescue::RescueEngine;
use crate::misc::utils::format_ether;
use crate::node::operations::ChangePubKeyOp;
use crate::params::{JUBJUB_PARAMS, RESCUE_PARAMS};
use crate::primitives::{pedersen_hash_tx_msg, rescue_hash_tx_msg, u128_to_bigdecimal};
use failure::{bail, ensure, format_err};
use parity_crypto::publickey::{
    public_to_address, recover, sign, KeyPair, Signature as ETHSignature,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::convert::TryInto;
use std::fmt;
use std::str::FromStr;
use web3::types::{Address, H256};

#[derive(Debug, Clone, PartialEq, Default, Eq, Hash, PartialOrd, Ord)]
pub struct TxHash {
    data: [u8; 32],
}

impl AsRef<[u8]> for TxHash {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl ToString for TxHash {
    fn to_string(&self) -> String {
        format!("sync-tx:{}", hex::encode(&self.data))
    }
}

impl FromStr for TxHash {
    type Err = failure::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ensure!(
            s.starts_with("sync-tx:"),
            "TxHash should start with sync-tx:"
        );
        let bytes = hex::decode(&s[8..])?;
        ensure!(bytes.len() == 32, "Size mismatch");
        Ok(TxHash {
            data: bytes.as_slice().try_into().unwrap(),
        })
    }
}

impl Serialize for TxHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TxHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        String::deserialize(deserializer).and_then(|string| {
            Self::from_str(&string).map_err(|err| Error::custom(err.to_string()))
        })
    }
}

/// Stores precomputed signature verification result to speedup tx execution
#[derive(Debug, Clone)]
enum VerifiedSignatureCache {
    /// No cache scenario
    NotCached,
    /// Cached: None if signature is incorrect.
    Cached(Option<PubKeyHash>),
}

impl Default for VerifiedSignatureCache {
    fn default() -> Self {
        Self::NotCached
    }
}

/// Signed by user.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub account_id: AccountId,
    pub from: Address,
    pub to: Address,
    pub token_id: TokenId,
    #[serde(skip)]
    pub fee: BigDecimal,
    pub nonce: Nonce,
    pub signature: TxSignature,
    #[serde(skip)]
    cached_signer: VerifiedSignatureCache,
}

impl Transfer {
    const TX_TYPE: u8 = 5;

    #[allow(clippy::too_many_arguments)]
    /// Creates transaction from parts
    /// signature is optional, because sometimes we don't know it (i.e. data_restore)
    pub fn new(
        account_id: AccountId,
        from: Address,
        to: Address,
        token_id: TokenId,
        fee: BigDecimal,
        nonce: Nonce,
        signature: Option<TxSignature>,
    ) -> Self {
        let mut tx = Self {
            account_id,
            from,
            to,
            token_id,
            fee,
            nonce,
            signature: signature.clone().unwrap_or_default(),
            cached_signer: VerifiedSignatureCache::NotCached,
        };
        if signature.is_some() {
            tx.cached_signer = VerifiedSignatureCache::Cached(tx.verify_signature());
        }
        tx
    }

    #[allow(clippy::too_many_arguments)]
    /// Creates signed transaction using private key, checks for correcteness
    pub fn new_signed(
        account_id: AccountId,
        from: Address,
        to: Address,
        token_id: TokenId,
        fee: BigDecimal,
        nonce: Nonce,
        private_key: &PrivateKey<Engine>,
    ) -> Result<Self, failure::Error> {
        let mut tx = Self::new(account_id, from, to, token_id, fee, nonce, None);
        tx.signature = TxSignature::sign_musig(private_key, &tx.get_bytes());
        if !tx.check_correctness() {
            bail!("Transfer is incorrect, check amounts");
        }
        Ok(tx)
    }

    pub fn get_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[Self::TX_TYPE]);
        out.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        out.extend_from_slice(&self.from.as_bytes());
        out.extend_from_slice(&self.to.as_bytes());
        out.extend_from_slice(&self.token_id.to_be_bytes());
        //out.extend_from_slice(&pack_fee_amount(&self.fee));
        out.extend_from_slice(&self.nonce.to_be_bytes());
        out
    }

    pub fn check_correctness(&mut self) -> bool {
        let mut valid = self.fee.is_integer() && is_fee_amount_packable(&self.fee);
        if valid {
            let signer = self.verify_signature();
            valid = valid && signer.is_some();
            self.cached_signer = VerifiedSignatureCache::Cached(signer);
        };
        valid
    }

    pub fn verify_signature(&self) -> Option<PubKeyHash> {
        if let VerifiedSignatureCache::Cached(cached_signer) = &self.cached_signer {
            cached_signer.clone()
        } else if let Some(pub_key) = self.signature.verify_musig(&self.get_bytes()) {
            Some(PubKeyHash::from_pubkey(&pub_key))
        } else {
            None
        }
    }

    /// Get message that should be signed by Ethereum keys of the account for 2F authentication.
    pub fn get_ethereum_sign_message(&self) -> String {
        format!(
            "Transfer {token_id}\n\
            To: {to:?}\n\
            Nonce: {nonce}\n\
            Account Id: {account_id}",
            token_id = self.token_id,
            to = self.to,
            nonce = self.nonce,
            account_id = self.account_id,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Withdraw {
    pub account_id: AccountId,
    pub from: Address,
    pub to: Address,
    pub token_id: TokenId,
    #[serde(skip)]
    pub fee: BigDecimal,
    pub nonce: Nonce,
    pub signature: TxSignature,
    #[serde(skip)]
    cached_signer: VerifiedSignatureCache,
}

impl Withdraw {
    const TX_TYPE: u8 = 3;

    #[allow(clippy::too_many_arguments)]
    /// Creates transaction from parts
    /// signature is optional, because sometimes we don't know it (i.e. data_restore)
    pub fn new(
        account_id: AccountId,
        from: Address,
        to: Address,
        token_id: TokenId,
        fee: BigDecimal,
        nonce: Nonce,
        signature: Option<TxSignature>,
    ) -> Self {
        let mut tx = Self {
            account_id,
            from,
            to,
            token_id,
            fee,
            nonce,
            signature: signature.clone().unwrap_or_default(),
            cached_signer: VerifiedSignatureCache::NotCached,
        };
        if signature.is_some() {
            tx.cached_signer = VerifiedSignatureCache::Cached(tx.verify_signature());
        }
        tx
    }

    #[allow(clippy::too_many_arguments)]
    /// Creates signed transaction using private key, checks for correcteness
    pub fn new_signed(
        account_id: AccountId,
        from: Address,
        to: Address,
        token_id: TokenId,
        fee: BigDecimal,
        nonce: Nonce,
        private_key: &PrivateKey<Engine>,
    ) -> Result<Self, failure::Error> {
        let mut tx = Self::new(account_id, from, to, token_id, fee, nonce, None);
        tx.signature = TxSignature::sign_musig(private_key, &tx.get_bytes());
        if !tx.check_correctness() {
            bail!("Transfer is incorrect, check amounts");
        }
        Ok(tx)
    }

    pub fn get_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[Self::TX_TYPE]);
        out.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        out.extend_from_slice(&self.from.as_bytes());
        out.extend_from_slice(self.to.as_bytes());
        out.extend_from_slice(&self.token_id.to_be_bytes());
        out.extend_from_slice(&self.nonce.to_be_bytes());
        out
    }

    pub fn check_correctness(&mut self) -> bool {
        let mut valid = self.fee.is_integer() && is_fee_amount_packable(&self.fee);

        if valid {
            let signer = self.verify_signature();
            valid = valid && signer.is_some();
            self.cached_signer = VerifiedSignatureCache::Cached(signer);
        }
        valid
    }

    pub fn verify_signature(&self) -> Option<PubKeyHash> {
        if let VerifiedSignatureCache::Cached(cached_signer) = &self.cached_signer {
            cached_signer.clone()
        } else if let Some(pub_key) = self.signature.verify_musig(&self.get_bytes()) {
            Some(PubKeyHash::from_pubkey(&pub_key))
        } else {
            None
        }
    }

    /// Get message that should be signed by Ethereum keys of the account for 2F authentication.
    pub fn get_ethereum_sign_message(&self) -> String {
        format!(
            "Withdraw {token_id}\n\
            To: {to:?}\n\
            Nonce: {nonce}\n\
            Account Id: {account_id}",
            token_id = self.token_id,
            to = self.to,
            nonce = self.nonce,
            account_id = self.account_id,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Close {
    pub account: Address,
    pub nonce: Nonce,
    pub signature: TxSignature,
}

impl Close {
    const TX_TYPE: u8 = 4;

    pub fn get_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[Self::TX_TYPE]);
        out.extend_from_slice(&self.account.as_bytes());
        out.extend_from_slice(&self.nonce.to_be_bytes());
        out
    }

    pub fn verify_signature(&self) -> Option<PubKeyHash> {
        if let Some(pub_key) = self.signature.verify_musig_rescue(&self.get_bytes()) {
            Some(PubKeyHash::from_pubkey(&pub_key))
        } else {
            None
        }
    }

    pub fn check_correctness(&self) -> bool {
        self.verify_signature().is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePubKey {
    pub account_id: AccountId,
    pub account: Address,
    pub new_pk_hash: PubKeyHash,
    pub nonce: Nonce,
    pub eth_signature: Option<PackedEthSignature>,
}

impl ChangePubKey {
    const TX_TYPE: u8 = 7;

    /// GetBytes for this transaction is used for hashing.
    pub fn get_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[Self::TX_TYPE]);
        out.extend_from_slice(&self.account_id.to_be_bytes()[1..]);
        out.extend_from_slice(&self.account.as_bytes());
        out.extend_from_slice(&self.new_pk_hash.data);
        out.extend_from_slice(&self.nonce.to_be_bytes());
        if let Some(sign) = &self.eth_signature {
            out.extend_from_slice(&sign.serialize_packed())
        }
        out
    }

    pub fn get_eth_signed_data(
        account_id: AccountId,
        nonce: Nonce,
        new_pubkey_hash: &PubKeyHash,
    ) -> Result<Vec<u8>, failure::Error> {
        const CHANGE_PUBKEY_SIGNATURE_LEN: usize = 150;
        let mut eth_signed_msg = Vec::with_capacity(CHANGE_PUBKEY_SIGNATURE_LEN);
        eth_signed_msg.extend_from_slice(b"Register zkSync pubkey:\n\n");
        eth_signed_msg.extend_from_slice(
            format!(
                "{}\n\
                 nonce: 0x{}\n\
                 account id: 0x{}\
                 \n\n",
                hex::encode(&new_pubkey_hash.data).to_ascii_lowercase(),
                hex::encode(&nonce.to_be_bytes()).to_ascii_lowercase(),
                hex::encode(&account_id.to_be_bytes()[1..]).to_ascii_lowercase()
            )
            .as_bytes(),
        );
        eth_signed_msg.extend_from_slice(b"Only sign this message for a trusted client!");
        ensure!(
            eth_signed_msg.len() == CHANGE_PUBKEY_SIGNATURE_LEN,
            "Change pubkey signed message len is too big: {}, expected: {}",
            eth_signed_msg.len(),
            CHANGE_PUBKEY_SIGNATURE_LEN
        );
        Ok(eth_signed_msg)
    }

    pub fn verify_eth_signature(&self) -> Option<Address> {
        self.eth_signature.as_ref().and_then(|sign| {
            Self::get_eth_signed_data(self.account_id, self.nonce, &self.new_pk_hash)
                .ok()
                .and_then(|msg| sign.signature_recover_signer(&msg).ok())
        })
    }

    pub fn check_correctness(&self) -> bool {
        self.eth_signature.is_none() || self.verify_eth_signature() == Some(self.account)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FranklinTx {
    Transfer(Box<Transfer>),
    Withdraw(Box<Withdraw>),
    Close(Box<Close>),
    ChangePubKey(Box<ChangePubKey>),
}

impl FranklinTx {
    pub fn hash(&self) -> TxHash {
        let bytes = match self {
            FranklinTx::Transfer(tx) => tx.get_bytes(),
            FranklinTx::Withdraw(tx) => tx.get_bytes(),
            FranklinTx::Close(tx) => tx.get_bytes(),
            FranklinTx::ChangePubKey(tx) => tx.get_bytes(),
        };

        let mut hasher = Sha256::new();
        hasher.input(&bytes);
        let mut out = [0u8; 32];
        hasher.result(&mut out);
        TxHash { data: out }
    }

    pub fn account(&self) -> Address {
        match self {
            FranklinTx::Transfer(tx) => tx.from,
            FranklinTx::Withdraw(tx) => tx.from,
            FranklinTx::Close(tx) => tx.account,
            FranklinTx::ChangePubKey(tx) => tx.account,
        }
    }

    pub fn nonce(&self) -> Nonce {
        match self {
            FranklinTx::Transfer(tx) => tx.nonce,
            FranklinTx::Withdraw(tx) => tx.nonce,
            FranklinTx::Close(tx) => tx.nonce,
            FranklinTx::ChangePubKey(tx) => tx.nonce,
        }
    }

    pub fn check_correctness(&mut self) -> bool {
        match self {
            FranklinTx::Transfer(tx) => tx.check_correctness(),
            FranklinTx::Withdraw(tx) => tx.check_correctness(),
            FranklinTx::Close(tx) => tx.check_correctness(),
            FranklinTx::ChangePubKey(tx) => tx.check_correctness(),
        }
    }

    pub fn get_bytes(&self) -> Vec<u8> {
        match self {
            FranklinTx::Transfer(tx) => tx.get_bytes(),
            FranklinTx::Withdraw(tx) => tx.get_bytes(),
            FranklinTx::Close(tx) => tx.get_bytes(),
            FranklinTx::ChangePubKey(tx) => tx.get_bytes(),
        }
    }

    pub fn min_chunks(&self) -> usize {
        match self {
            FranklinTx::Transfer(_) => TransferOp::CHUNKS,
            FranklinTx::Withdraw(_) => WithdrawOp::CHUNKS,
            FranklinTx::Close(_) => CloseOp::CHUNKS,
            FranklinTx::ChangePubKey(_) => ChangePubKeyOp::CHUNKS,
        }
    }

    pub fn is_withdraw(&self) -> bool {
        match self {
            FranklinTx::Withdraw(_) => true,
            _ => false,
        }
    }

    pub fn is_close(&self) -> bool {
        match self {
            FranklinTx::Close(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxSignature {
    pub pub_key: PackedPublicKey,
    pub signature: PackedSignature,
}

impl TxSignature {
    pub fn sign_musig(pk: &PrivateKey<Engine>, msg: &[u8]) -> Self {
        Self::sign_musig_rescue(pk, msg)
    }

    pub fn verify_musig(&self, msg: &[u8]) -> Option<PublicKey<Engine>> {
        self.verify_musig_rescue(msg)
    }

    #[allow(dead_code)]
    fn verify_musig_pedersen(&self, msg: &[u8]) -> Option<PublicKey<Engine>> {
        let hashed_msg = pedersen_hash_tx_msg(msg);
        let valid = self.pub_key.0.verify_musig_pedersen(
            &hashed_msg,
            &self.signature.0,
            FixedGenerators::SpendingKeyGenerator,
            &JUBJUB_PARAMS,
        );
        if valid {
            Some(self.pub_key.0.clone())
        } else {
            None
        }
    }

    #[allow(dead_code)]
    fn verify_musig_sha256(&self, msg: &[u8]) -> Option<PublicKey<Engine>> {
        let hashed_msg = pedersen_hash_tx_msg(msg);
        let valid = self.pub_key.0.verify_musig_sha256(
            &hashed_msg,
            &self.signature.0,
            FixedGenerators::SpendingKeyGenerator,
            &JUBJUB_PARAMS,
        );
        if valid {
            Some(self.pub_key.0.clone())
        } else {
            None
        }
    }

    fn verify_musig_rescue(&self, msg: &[u8]) -> Option<PublicKey<Engine>> {
        let hashed_msg = rescue_hash_tx_msg(msg);
        let valid = self.pub_key.0.verify_musig_rescue(
            &hashed_msg,
            &self.signature.0,
            FixedGenerators::SpendingKeyGenerator,
            &RESCUE_PARAMS,
            &JUBJUB_PARAMS,
        );
        if valid {
            Some(self.pub_key.0.clone())
        } else {
            None
        }
    }

    #[allow(dead_code)]
    fn sign_musig_pedersen(pk: &PrivateKey<Engine>, msg: &[u8]) -> Self {
        let hashed_msg = pedersen_hash_tx_msg(msg);
        let seed = Seed::deterministic_seed(&pk, &hashed_msg);
        let signature = pk.musig_pedersen_sign(
            &hashed_msg,
            &seed,
            FixedGenerators::SpendingKeyGenerator,
            &JUBJUB_PARAMS,
        );

        Self {
            pub_key: PackedPublicKey(public_key_from_private(pk)),
            signature: PackedSignature(signature),
        }
    }

    #[allow(dead_code)]
    fn sign_musig_sha256(pk: &PrivateKey<Engine>, msg: &[u8]) -> Self {
        let hashed_msg = pedersen_hash_tx_msg(msg);
        let seed = Seed::deterministic_seed(&pk, &hashed_msg);
        let signature = pk.musig_sha256_sign(
            &hashed_msg,
            &seed,
            FixedGenerators::SpendingKeyGenerator,
            &JUBJUB_PARAMS,
        );

        Self {
            pub_key: PackedPublicKey(public_key_from_private(pk)),
            signature: PackedSignature(signature),
        }
    }

    fn sign_musig_rescue(pk: &PrivateKey<Engine>, msg: &[u8]) -> Self
    where
        Engine: RescueEngine,
    {
        let hashed_msg = rescue_hash_tx_msg(msg);
        let seed = Seed::deterministic_seed(&pk, &hashed_msg);
        let signature = pk.musig_rescue_sign(
            &hashed_msg,
            &seed,
            FixedGenerators::SpendingKeyGenerator,
            &RESCUE_PARAMS,
            &JUBJUB_PARAMS,
        );

        Self {
            pub_key: PackedPublicKey(public_key_from_private(pk)),
            signature: PackedSignature(signature),
        }
    }

    /// Deserialize signature from packed bytes representation.
    /// [0..32] - packed pubkey of the signer.
    /// [32..96] - packed r,s of the signature
    pub fn deserialize_from_packed_bytes(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(bytes.len() == 32 + 64, "packed signature length mismatch");
        Ok(Self {
            pub_key: PackedPublicKey::deserialize_packed(&bytes[0..32])?,
            signature: PackedSignature::deserialize_packed(&bytes[32..])?,
        })
    }
}

impl Default for TxSignature {
    fn default() -> Self {
        Self {
            pub_key: PackedPublicKey::deserialize_packed(&[0; 32]).unwrap(),
            signature: PackedSignature::deserialize_packed(&[0; 64]).unwrap(),
        }
    }
}

impl std::fmt::Debug for TxSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let hex_pk = hex::encode(&self.pub_key.serialize_packed().unwrap());
        let hex_sign = hex::encode(&self.signature.serialize_packed().unwrap());
        write!(f, "{{ pub_key: {}, sign: {} }}", hex_pk, hex_sign)
    }
}

#[derive(Clone)]
pub struct PackedPublicKey(pub PublicKey<Engine>);

impl PackedPublicKey {
    pub fn serialize_packed(&self) -> std::io::Result<Vec<u8>> {
        let mut packed_point = [0u8; 32];
        (self.0).0.write(packed_point.as_mut())?;
        Ok(packed_point.to_vec())
    }

    pub fn deserialize_packed(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(bytes.len() == 32, "PublicKey size mismatch");

        Ok(PackedPublicKey(PublicKey::<Engine>(
            edwards::Point::read(&*bytes, &JUBJUB_PARAMS as &AltJubjubBn256)
                .map_err(|e| format_err!("Failed to restore point: {}", e.to_string()))?,
        )))
    }
}

impl Serialize for PackedPublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::Error;
        let packed_point = self
            .serialize_packed()
            .map_err(|e| Error::custom(e.to_string()))?;

        serializer.serialize_str(&hex::encode(packed_point))
    }
}

impl<'de> Deserialize<'de> for PackedPublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        String::deserialize(deserializer).and_then(|string| {
            let bytes = hex::decode(&string).map_err(|e| Error::custom(e.to_string()))?;
            PackedPublicKey::deserialize_packed(&bytes).map_err(|e| Error::custom(e.to_string()))
        })
    }
}

#[derive(Clone)]
pub struct PackedSignature(pub Signature<Engine>);

impl PackedSignature {
    pub fn serialize_packed(&self) -> std::io::Result<Vec<u8>> {
        let mut packed_signature = [0u8; 64];
        let (r_bar, s_bar) = packed_signature.as_mut().split_at_mut(32);

        (self.0).r.write(r_bar)?;
        (self.0).s.into_repr().write_le(s_bar)?;

        Ok(packed_signature.to_vec())
    }

    pub fn deserialize_packed(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(bytes.len() == 64, "Signature size mismatch");
        let (r_bar, s_bar) = bytes.split_at(32);

        let r = edwards::Point::read(r_bar, &JUBJUB_PARAMS as &AltJubjubBn256)
            .map_err(|e| format_err!("Failed to restore R point from R_bar: {}", e.to_string()))?;

        let mut s_repr = FsRepr::default();
        s_repr
            .read_le(s_bar)
            .map_err(|e| format_err!("s read err: {}", e.to_string()))?;

        let s = <Engine as JubjubEngine>::Fs::from_repr(s_repr)
            .map_err(|e| format_err!("Failed to restore s scalar from s_bar: {}", e.to_string()))?;

        Ok(Self(Signature { r, s }))
    }
}

impl Serialize for PackedSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::Error;

        let packed_signature = self
            .serialize_packed()
            .map_err(|e| Error::custom(e.to_string()))?;
        serializer.serialize_str(&hex::encode(&packed_signature))
    }
}

impl<'de> Deserialize<'de> for PackedSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        String::deserialize(deserializer).and_then(|string| {
            let bytes = hex::decode(&string).map_err(|e| Error::custom(e.to_string()))?;
            PackedSignature::deserialize_packed(&bytes).map_err(|e| Error::custom(e.to_string()))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "signature")]
pub enum TxEthSignature {
    EthereumSignature(PackedEthSignature),
    EIP1271Signature(EIP1271Signature),
}

#[derive(Debug, Clone)]
pub struct EIP1271Signature(pub Vec<u8>);

impl fmt::Display for EIP1271Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EIP1271Signature 0x{}", hex::encode(&self.0.as_slice()))
    }
}

impl<'de> Deserialize<'de> for EIP1271Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use hex::FromHex;
        use serde::de::Error;

        let string = String::deserialize(deserializer)?;

        if !string.starts_with("0x") {
            return Err(Error::custom("Packed eth signature should start with 0x"));
        }

        Vec::from_hex(&string[2..])
            .map(Self)
            .map_err(|err| Error::custom(err.to_string()))
    }
}

impl Serialize for EIP1271Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("0x{}", &hex::encode(self.0.as_slice())))
    }
}

/// Struct used for working with ethereum signatures created using eth_sign (using geth, ethers.js, etc)
/// message is serialized as 65 bytes long `0x` prefixed string.
///
/// Some notes on implementation of methods of this structure:
///
/// Ethereum signed messages expect v parameter to be 27 + recovery_id(0,1,2,3)
/// Library that we use for signature verification (written for bitcoin) expects v = recovery_id
///
/// That is why:
/// 1) when we create this structure by deserialization of message produced by user
/// we subtract 27 from v in `ETHSignature` and store it in ETHSignature structure this way.
/// 2) When we serialize/create this structure we add 27 to v in `ETHSignature`.
///
/// This way when we have methods that consumes &self we can be sure that ETHSignature::recover_signer works
/// And we can be sure that we are compatible with Ethereum clients.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedEthSignature(ETHSignature);

impl PackedEthSignature {
    pub fn serialize_packed(&self) -> [u8; 65] {
        // adds 27 to v
        self.0.clone().into_electrum()
    }

    pub fn deserialize_packed(bytes: &[u8]) -> Result<Self, failure::Error> {
        ensure!(bytes.len() == 65, "eth signature length should be 65 bytes");
        // assumes v = recover + 27
        Ok(PackedEthSignature(ETHSignature::from_electrum(&bytes)))
    }

    /// Signs message using ethereum private key, results are identical to signature created
    /// using `geth`, `ethers.js`, etc. No hashing and prefixes required.
    pub fn sign(private_key: &H256, msg: &[u8]) -> Result<PackedEthSignature, failure::Error> {
        let secret_key = (*private_key).into();
        let signed_bytes = Self::message_to_signed_bytes(msg);
        let signature = sign(&secret_key, &signed_bytes)?;
        Ok(PackedEthSignature(signature))
    }

    fn message_to_signed_bytes(msg: &[u8]) -> H256 {
        let prefix = format!("\x19Ethereum Signed Message:\n{}", msg.len());
        let mut bytes = Vec::with_capacity(prefix.len() + msg.len());
        bytes.extend_from_slice(prefix.as_bytes());
        bytes.extend_from_slice(msg);
        tiny_keccak::keccak256(&bytes).into()
    }

    /// Checks signature and returns ethereum address of the signer.
    /// message should be the same message that was passed to `eth.sign`(or similar) method
    /// as argument. No hashing and prefixes required.
    pub fn signature_recover_signer(&self, msg: &[u8]) -> Result<Address, failure::Error> {
        let signed_bytes = Self::message_to_signed_bytes(msg);
        let public_key = recover(&self.0, &signed_bytes)?;
        Ok(public_to_address(&public_key))
    }

    /// Get Ethereum address from private key,
    pub fn address_from_private_key(private_key: &H256) -> Result<Address, failure::Error> {
        Ok(KeyPair::from_secret((*private_key).into())?.address())
    }
}

impl Serialize for PackedEthSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let packed_signature = self.serialize_packed();
        serializer.serialize_str(&format!("0x{}", &hex::encode(&packed_signature[..])))
    }
}

impl<'de> Deserialize<'de> for PackedEthSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        String::deserialize(deserializer).and_then(|string| {
            if !string.starts_with("0x") {
                return Err(Error::custom("Packed eth signature should start with 0x"));
            }
            let bytes = hex::decode(&string[2..]).map_err(|e| Error::custom(e.to_string()))?;
            PackedEthSignature::deserialize_packed(&bytes).map_err(|e| Error::custom(e.to_string()))
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rand::{Rng, SeedableRng, XorShiftRng};

    fn gen_pk_and_msg() -> (PrivateKey<Engine>, Vec<Vec<u8>>) {
        let mut rng = XorShiftRng::from_seed([1, 2, 3, 4]);

        let pk = PrivateKey(rng.gen());

        let mut messages = Vec::new();
        messages.push(Vec::<u8>::new());
        messages.push(b"hello world".to_vec());

        (pk, messages)
    }

    fn gen_account_id<T: Rng>(rng: &mut T) -> AccountId {
        let mut bytes = rng.gen::<u32>().to_be_bytes();
        bytes[0] = 0;
        u32::from_be_bytes(bytes)
    }

    #[test]
    fn test_print_transfer_for_protocol() {
        let mut rng = XorShiftRng::from_seed([1, 2, 3, 4]);
        let key = gen_pk_and_msg().0;
        let transfer = Transfer::new_signed(
            gen_account_id(&mut rng),
            Address::from(rng.gen::<[u8; 20]>()),
            Address::from(rng.gen::<[u8; 20]>()),
            rng.gen(),
            BigDecimal::from(56_700_000_000u64),
            rng.gen(),
            &key,
        )
        .expect("failed to sign transfer");

        println!(
            "User representation:\n{}\n",
            serde_json::to_string_pretty(&transfer).expect("json serialize")
        );

        println!("Signer:");
        println!("Private key: {}", key.0.to_string());
        let (pk_x, pk_y) = public_key_from_private(&key).0.into_xy();
        println!("Public key: x: {}, y: {}\n", pk_x, pk_y);

        let signed_fields = vec![
            ("type", vec![Transfer::TX_TYPE]),
            ("accountId", transfer.account_id.to_be_bytes()[1..].to_vec()),
            ("from", transfer.from.as_bytes().to_vec()),
            ("to", transfer.to.as_bytes().to_vec()),
            ("token_id", transfer.token_id.to_be_bytes().to_vec()),
            ("fee", pack_fee_amount(&transfer.fee)),
            ("nonce", transfer.nonce.to_be_bytes().to_vec()),
        ];
        println!("Signed transaction fields:");
        let mut field_concat = Vec::new();
        for (field, value) in signed_fields.into_iter() {
            println!("{}: 0x{}", field, hex::encode(&value));
            field_concat.extend(value.into_iter());
        }
        println!("Signed bytes: 0x{}", hex::encode(&field_concat));
        assert_eq!(
            field_concat,
            transfer.get_bytes(),
            "Protocol serialization mismatch"
        );
    }

    #[test]
    fn test_print_withdraw_for_protocol() {
        let mut rng = XorShiftRng::from_seed([2, 2, 3, 4]);
        let key = gen_pk_and_msg().0;
        let withdraw = Withdraw::new_signed(
            gen_account_id(&mut rng),
            Address::from(rng.gen::<[u8; 20]>()),
            Address::from(rng.gen::<[u8; 20]>()),
            rng.gen(),
            BigDecimal::from(56_700_000_000u64),
            rng.gen(),
            &key,
        )
        .expect("failed to sign withdraw");

        println!(
            "User representation:\n{}\n",
            serde_json::to_string_pretty(&withdraw).expect("json serialize")
        );

        println!("Signer:");
        println!("Private key: {}", key.0.to_string());
        let (pk_x, pk_y) = public_key_from_private(&key).0.into_xy();
        println!("Public key: x: {}, y: {}\n", pk_x, pk_y);

        let signed_fields = vec![
            ("type", vec![Withdraw::TX_TYPE]),
            ("accountId", withdraw.account_id.to_be_bytes()[1..].to_vec()),
            ("from", withdraw.from.as_bytes().to_vec()),
            ("to", withdraw.to.as_bytes().to_vec()),
            ("token_id", withdraw.token_id.to_be_bytes().to_vec()),
            ("fee", pack_fee_amount(&withdraw.fee)),
            ("nonce", withdraw.nonce.to_be_bytes().to_vec()),
        ];
        println!("Signed transaction fields:");
        let mut field_concat = Vec::new();
        for (field, value) in signed_fields.into_iter() {
            println!("{}: 0x{}", field, hex::encode(&value));
            field_concat.extend(value.into_iter());
        }
        println!("Signed bytes: 0x{}", hex::encode(&field_concat));
        assert_eq!(
            field_concat,
            withdraw.get_bytes(),
            "Protocol serialization mismatch"
        );
    }

    #[test]
    fn test_musig_rescue_signing_verification() {
        let (pk, messages) = gen_pk_and_msg();

        for msg in &messages {
            let signature = TxSignature::sign_musig_rescue(&pk, msg);

            if let Some(sign_pub_key) = signature.verify_musig_rescue(msg) {
                let pub_key = PublicKey::from_private(
                    &pk,
                    FixedGenerators::SpendingKeyGenerator,
                    &JUBJUB_PARAMS,
                );
                assert!(
                    sign_pub_key.0.eq(&pub_key.0),
                    "Signature pub key is wrong, msg: {}",
                    hex::encode(&msg)
                );
            } else {
                panic!("Signature is incorrect, msg: {}", hex::encode(&msg));
            }
        }
    }

    #[test]
    fn test_musig_pedersen_signing_verification() {
        let (pk, messages) = gen_pk_and_msg();

        for msg in &messages {
            let signature = TxSignature::sign_musig_pedersen(&pk, msg);

            if let Some(sign_pub_key) = signature.verify_musig_pedersen(msg) {
                let pub_key = PublicKey::from_private(
                    &pk,
                    FixedGenerators::SpendingKeyGenerator,
                    &JUBJUB_PARAMS,
                );
                assert!(
                    sign_pub_key.0.eq(&pub_key.0),
                    "Signature pub key is wrong, msg: {}",
                    hex::encode(&msg)
                );
            } else {
                panic!("Signature is incorrect, msg: {}", hex::encode(&msg));
            }
        }
    }

    #[test]
    fn test_musig_sha256_signing_verification() {
        let (pk, messages) = gen_pk_and_msg();

        for msg in &messages {
            let signature = TxSignature::sign_musig_sha256(&pk, msg);

            if let Some(sign_pub_key) = signature.verify_musig_sha256(msg) {
                let pub_key = PublicKey::from_private(
                    &pk,
                    FixedGenerators::SpendingKeyGenerator,
                    &JUBJUB_PARAMS,
                );
                assert!(
                    sign_pub_key.0.eq(&pub_key.0),
                    "Signature pub key is wrong, msg: {}",
                    hex::encode(&msg)
                );
            } else {
                panic!("Signature is incorrect, msg: {}", hex::encode(&msg));
            }
        }
    }

    #[test]
    fn test_ethereum_signature_verify_with_serialization() {
        let address: Address = "52312AD6f01657413b2eaE9287f6B9ADaD93D5FE".parse().unwrap();
        let message = "hello world";

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct TestSignatureSerialize {
            signature: PackedEthSignature,
        }

        // signature calculated using ethers.js signer
        let test_signature_serialize = "{ \"signature\": \"0x111ea2824732851dd0893eaa5873597ba38ed08b69f6d8a0d7f5da810335566403d05281b1f56d12ca653e32eb7d67b76814b0cc8b0da2d7ad2c862d575329951b\"}";

        // test serialization
        let deserialized_signature: TestSignatureSerialize =
            serde_json::from_str(test_signature_serialize).expect("signature deserialize");
        let signature_after_roundtrip: TestSignatureSerialize = serde_json::from_str(
            &serde_json::to_string(&deserialized_signature).expect("signature serialize roundtrip"),
        )
        .expect("signature deserialize roundtrip");
        assert_eq!(
            deserialized_signature, signature_after_roundtrip,
            "signature serialize-deserialize roundtrip"
        );

        let recovered_address = deserialized_signature
            .signature
            .signature_recover_signer(message.as_bytes())
            .expect("signature verification");

        assert_eq!(address, recovered_address, "recovered address mismatch");
    }

    #[test]
    fn test_ethereum_signature_verify_examples() {
        // signatures created using geth
        // e.g. in geth console: eth.sign(eth.accounts[0], "0x")
        let examples = vec![
            ("0x8a91dc2d28b689474298d91899f0c1baf62cb85b", "0xdead", "0x13c34c76ffb42d97da67ddc5d275e92d758d1b48b5ee4b3bacd800cbeec3baff043a5ee63fea55485e1ee5d6f8b088daabd095f2ebbdc80a33806528b44bfccc1c"),
            // empty message
            ("0x8a91dc2d28b689474298d91899f0c1baf62cb85b", "0x", "0xd98f51c2ee0fd589e421348002dffec5d1b38e5bef9a41a699030456dc39298d12698158dc2a814b5f9ac6d433009dec87484a4579107be3f8f33907e92938291b"),
            // this example has v = 28, unlike others
            ("0x8a91dc2d28b689474298d91899f0c1baf62cb85b", "0x14", "0xd288b623af654c9d805e132812edf09ce244040376ca49112e91d437ecceed7c518690d4ae14149cd533f1ca4f081e6d2252c980fccc63de4d6bb818f1b668921c"),
        ];

        for (address, msg, signature) in examples {
            println!("addr: {}, msg: {}, sign: {}", address, msg, signature);
            let address = address[2..].parse::<Address>().unwrap();
            let msg = hex::decode(&msg[2..]).unwrap();
            let signature =
                PackedEthSignature::deserialize_packed(&hex::decode(&signature[2..]).unwrap())
                    .expect("signature deserialize");
            let signer_address = signature
                .signature_recover_signer(&msg)
                .expect("signature verification");
            assert_eq!(address, signer_address, "signer address mismatch");
        }
    }

    #[test]
    fn test_ethereum_signature_sign() {
        // data generated with `ethers.js`
        let private_key = "0b43c0f5b5a13a7047408d1f8c8ad32ba5879902ea6212184e0a5d1157281d76"
            .parse()
            .unwrap();

        let examples = vec![
            (b"hello world".to_vec(), "12c24491eefbac7e80f4d3f0400cd804667dab026fda1bc8bfe86650d872ba4215b0a0e297c48a54d9020daa3130222dadcb8f5ffdafc4b9293c3ef818b322b01c"),
            // empty message
            (Vec::new(), "8b7385c7bb8913b9fd176247efab0ccc72e3197abe8e2d4c6596ba58a32a91675f66e80560a5f1a42bd50d58da055630ac6c18875e5ba14a362e87e903f083941c"),
            // v = 27(others v = 28)
            (vec![0x12, 0x32, 0x12, 0x42], "463d955775a407eadfdb22437d53df42460977bf1c02cf830b579b6bd0000ff366e819af75fb7140e8797d56580acfcac0ad3567bbdeca118a5f5d37f09753f11b")
        ];
        for (msg, correct_signature) in examples {
            println!("message: 0x{}", hex::encode(&msg));
            let correct_signature = hex::decode(correct_signature).unwrap();
            let signature = PackedEthSignature::sign(&private_key, &msg)
                .expect("sign verify")
                .serialize_packed()
                .to_vec();
            assert_eq!(signature, correct_signature, "signature is incorrect");
        }
    }
}
