// External deps
use crypto_exports::franklin_crypto::bellman::pairing::{
    bn256::{Bn256, Fr},
    ff::{Field, PrimeField},
};
// Workspace deps
use models::circuit::{account::CircuitAccountTree, utils::le_bit_vector_into_field_element};
// Local deps
use crate::{
    account::AccountWitness,
    operation::{
        Operation, OperationArguments, OperationBranch, OperationBranchWitness, SignatureData,
    },
    witness::utils::get_audits,
};

pub fn noop_operation(tree: &CircuitAccountTree, acc_id: u32) -> Operation<Bn256> {
    let signature_data = SignatureData::init_empty();
    let first_sig_msg = Fr::zero();
    let second_sig_msg = Fr::zero();
    let third_sig_msg = Fr::zero();
    let signer_pub_key_packed = [Some(false); 256];

    let acc = tree.get(acc_id).unwrap();
    let account_address_fe = Fr::from_str(&acc_id.to_string()).unwrap();
    let pubdata = vec![false; 64];
    let pubdata_chunks: Vec<_> = pubdata
        .chunks(64)
        .map(|x| le_bit_vector_into_field_element(&x.to_vec()))
        .collect();
    let (audit_account, audit_token) = get_audits(tree, acc_id, 0);

    Operation {
        new_root: Some(tree.root_hash()),
        tx_type: Some(Fr::from_str("0").unwrap()),
        chunk: Some(Fr::from_str("0").unwrap()),
        pubdata_chunk: Some(pubdata_chunks[0]),
        first_sig_msg: Some(first_sig_msg),
        second_sig_msg: Some(second_sig_msg),
        third_sig_msg: Some(third_sig_msg),
        signature_data,
        signer_pub_key_packed: signer_pub_key_packed.to_vec(),

        args: OperationArguments {
            eth_address: Some(Fr::zero()),
            //amount_packed: Some(Fr::zero()),
            //full_amount: Some(Fr::zero()),
            fee: Some(Fr::zero()),
            //a: Some(Fr::zero()),
            //b: Some(Fr::zero()),
            pub_nonce: Some(Fr::zero()),
            new_pub_key_hash: Some(Fr::zero()),
            token_id: Some(Fr::zero()),
        },
        lhs: OperationBranch {
            address: Some(account_address_fe),
            witness: OperationBranchWitness {
                account_witness: AccountWitness {
                    nonce: Some(acc.nonce),
                    pub_key_hash: Some(acc.pub_key_hash),
                    address: Some(acc.address),
                },
                account_path: audit_account.clone(),
                //balance_value: Some(balance_value),
                token_subtree_path: audit_token.clone(),
            },
        },
        rhs: OperationBranch {
            address: Some(account_address_fe),
            witness: OperationBranchWitness {
                account_witness: AccountWitness {
                    nonce: Some(acc.nonce),
                    pub_key_hash: Some(acc.pub_key_hash),
                    address: Some(acc.address),
                },
                account_path: audit_account,
                //balance_value: Some(balance_value),
                token_subtree_path: audit_token,
            },
        },
    }
}
