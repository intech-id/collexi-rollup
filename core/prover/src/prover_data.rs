// Built-in
// External
use serde::{Deserialize, Serialize};
// Workspace
use circuit::account::AccountWitness;
use circuit::circuit::FranklinCircuit;
use circuit::operation::{
    OperationArguments, OperationBranch, OperationBranchWitness, SignatureData,
};
use crypto_exports::ff::PrimeField;
use crypto_exports::franklin_crypto::alt_babyjubjub::AltJubjubBn256;
use crypto_exports::franklin_crypto::rescue::bn256::Bn256RescueParams;
use models::node::{Engine, Fr};
// Local
use crate::serialization::*;

/// ProverData is data prover needs to calculate proof of the given block.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProverData {
    #[serde(with = "FrSerde")]
    pub public_data_commitment: Fr,
    #[serde(with = "FrSerde")]
    pub old_root: Fr,
    #[serde(with = "FrSerde")]
    pub new_root: Fr,
    #[serde(with = "FrSerde")]
    pub validator_address: Fr,
    #[serde(with = "VecOptionalFrSerde")]
    pub validator_balances: Vec<Option<Fr>>,
    #[serde(with = "VecOptionalFrSerde")]
    pub validator_audit_path: Vec<Option<Fr>>,
    #[serde(with = "VecOperationsSerde")]
    pub operations: Vec<circuit::operation::Operation<Engine>>,
    #[serde(with = "AccountWitnessDef")]
    pub validator_account: circuit::account::AccountWitness<Engine>,
}

impl ProverData {
    pub fn into_circuit(self, block: i64) -> FranklinCircuit<'static, Engine> {
        FranklinCircuit {
            rescue_params: &models::params::RESCUE_PARAMS as &Bn256RescueParams,
            jubjub_params: &models::params::JUBJUB_PARAMS as &AltJubjubBn256,
            old_root: Some(self.old_root),
            block_number: Fr::from_str(&block.to_string()),
            validator_address: Some(self.validator_address),
            pub_data_commitment: Some(self.public_data_commitment),
            operations: self.operations,
            validator_balances: self.validator_balances,
            validator_audit_path: self.validator_audit_path,
            validator_account: self.validator_account,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "circuit::account::AccountWitness::<Engine>")]
struct AccountWitnessDef {
    #[serde(with = "OptionalFrSerde")]
    pub nonce: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub pub_key_hash: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub address: Option<Fr>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "circuit::operation::Operation::<Engine>")]
pub struct OperationDef {
    #[serde(with = "OptionalFrSerde")]
    pub new_root: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub tx_type: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub chunk: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub pubdata_chunk: Option<Fr>,

    pub signer_pub_key_packed: Vec<Option<bool>>,
    #[serde(with = "OptionalFrSerde")]
    pub first_sig_msg: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub second_sig_msg: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub third_sig_msg: Option<Fr>,
    pub signature_data: SignatureData,
    #[serde(with = "OperationArgumentsDef")]
    pub args: OperationArguments<Engine>,
    #[serde(with = "OperationBranchDef")]
    pub lhs: OperationBranch<Engine>,
    #[serde(with = "OperationBranchDef")]
    pub rhs: OperationBranch<Engine>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "circuit::operation::OperationArguments::<Engine>")]
pub struct OperationArgumentsDef {
    //#[serde(with = "OptionalFrSerde")]
    //pub a: Option<Fr>,
    //#[serde(with = "OptionalFrSerde")]
    //pub b: Option<Fr>,
    //#[serde(with = "OptionalFrSerde")]
    //pub amount_packed: Option<Fr>,
    //#[serde(with = "OptionalFrSerde")]
    //pub full_amount: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub token_id: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub fee: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub new_pub_key_hash: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub eth_address: Option<Fr>,
    #[serde(with = "OptionalFrSerde")]
    pub pub_nonce: Option<Fr>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "circuit::operation::OperationBranch::<Engine>")]
pub struct OperationBranchDef {
    #[serde(with = "OptionalFrSerde")]
    pub address: Option<Fr>,
    //#[serde(with = "OptionalFrSerde")]
    //pub token: Option<Fr>,
    #[serde(with = "OperationBranchWitnessDef")]
    pub witness: OperationBranchWitness<Engine>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "circuit::operation::OperationBranchWitness::<Engine>")]
pub struct OperationBranchWitnessDef {
    #[serde(with = "AccountWitnessDef")]
    pub account_witness: AccountWitness<Engine>,
    #[serde(with = "VecOptionalFrSerde")]
    pub account_path: Vec<Option<Fr>>,
    //#[serde(with = "OptionalFrSerde")]
    //pub balance_value: Option<Fr>,
    #[serde(with = "VecOptionalFrSerde")]
    pub token_subtree_path: Vec<Option<Fr>>,
}
