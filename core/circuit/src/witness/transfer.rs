// External deps
use crypto_exports::franklin_crypto::{
    bellman::pairing::{
        bn256::{Bn256, Fr},
        ff::{Field, PrimeField},
    },
    rescue::RescueEngine,
};
// Workspace deps
use models::{
    circuit::{
        account::CircuitAccountTree,
        utils::{append_be_fixed_width, le_bit_vector_into_field_element},
    },
    node::operations::TransferOp,
    params as franklin_constants,
    primitives::{big_decimal_to_u128, convert_to_float},
};
// Local deps
use crate::{
    operation::{Operation, OperationArguments, OperationBranch, OperationBranchWitness},
    witness::{
        utils::{apply_leaf_operation, get_audits, SigDataInput},
        Witness,
    },
};

#[derive(Debug)]
pub struct TransferData {
    //pub amount: u128,
    pub fee: u128,
    pub token_id: u32,
    pub from_account_address: u32,
    pub to_account_address: u32,
}

pub struct TransferWitness<E: RescueEngine> {
    pub from_before: OperationBranch<E>,
    pub from_intermediate: OperationBranch<E>,
    pub from_after: OperationBranch<E>,
    pub to_before: OperationBranch<E>,
    pub to_intermediate: OperationBranch<E>,
    pub to_after: OperationBranch<E>,
    pub args: OperationArguments<E>,
    pub before_root: Option<E::Fr>,
    pub intermediate_root: Option<E::Fr>,
    pub after_root: Option<E::Fr>,
    pub tx_type: Option<E::Fr>,
}

impl Witness for TransferWitness<Bn256> {
    type OperationType = TransferOp;
    type CalculateOpsInput = SigDataInput;

    fn apply_tx(tree: &mut CircuitAccountTree, transfer: &TransferOp) -> Self {
        let transfer_data = TransferData {
            //amount: big_decimal_to_u128(&transfer.tx.amount),
            fee: big_decimal_to_u128(&transfer.tx.fee),
            token_id: u32::from(transfer.tx.token_id),
            from_account_address: transfer.from,
            to_account_address: transfer.to,
        };
        // le_bit_vector_into_field_element()
        Self::apply_data(tree, &transfer_data)
    }

    fn get_pubdata(&self) -> Vec<bool> {
        // construct pubdata
        let mut pubdata_bits = vec![];
        append_be_fixed_width(
            &mut pubdata_bits,
            &self.tx_type.unwrap(),
            franklin_constants::TX_TYPE_BIT_WIDTH,
        );

        append_be_fixed_width(
            &mut pubdata_bits,
            &self.from_before.address.unwrap(),
            franklin_constants::ACCOUNT_ID_BIT_WIDTH,
        );
        /*append_be_fixed_width(
            &mut pubdata_bits,
            &self.from_before.token.unwrap(),
            franklin_constants::TOKENID_BIT_WIDTH,
        );*/
        append_be_fixed_width(
            &mut pubdata_bits,
            &self.to_before.address.unwrap(),
            franklin_constants::ACCOUNT_ID_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut pubdata_bits,
            &self.args.token_id.unwrap(),
            franklin_constants::TOKENID_BIT_WIDTH,
        );

        append_be_fixed_width(
            &mut pubdata_bits,
            &self.args.fee.unwrap(),
            franklin_constants::FEE_MANTISSA_BIT_WIDTH + franklin_constants::FEE_EXPONENT_BIT_WIDTH,
        );
        pubdata_bits.resize(2 * franklin_constants::CHUNK_BIT_WIDTH, false); //TODO verify if right padding is okay
        pubdata_bits
    }

    fn calculate_operations(&self, input: SigDataInput) -> Vec<Operation<Bn256>> {
        let pubdata_chunks: Vec<_> = self
            .get_pubdata()
            .chunks(64)
            .map(|x| le_bit_vector_into_field_element(&x.to_vec()))
            .collect();

        let operation_zero = Operation {
            new_root: self.intermediate_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("0").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[0]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.from_before.clone(),
            rhs: self.to_before.clone(),
        };

        let operation_one = Operation {
            new_root: self.after_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("1").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[1]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.from_intermediate.clone(),
            rhs: self.to_intermediate.clone(),
        };
        vec![operation_zero, operation_one]
    }
}

impl<E: RescueEngine> TransferWitness<E> {
    pub fn get_sig_bits(&self) -> Vec<bool> {
        let mut sig_bits = vec![];
        append_be_fixed_width(
            &mut sig_bits,
            &Fr::from_str("5").unwrap(), //Corresponding tx_type
            franklin_constants::TX_TYPE_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self
                .from_before
                .witness
                .account_witness
                .pub_key_hash
                .unwrap(),
            franklin_constants::NEW_PUBKEY_HASH_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self.to_before.witness.account_witness.pub_key_hash.unwrap(),
            franklin_constants::NEW_PUBKEY_HASH_WIDTH,
        );

        /*append_be_fixed_width(
            &mut sig_bits,
            &self.from_before.token.unwrap(),
            franklin_constants::TOKEN_BIT_WIDTH,
        );*/
        /*append_be_fixed_width(
            &mut sig_bits,
            &self.args.amount_packed.unwrap(),
            franklin_constants::AMOUNT_MANTISSA_BIT_WIDTH
                + franklin_constants::AMOUNT_EXPONENT_BIT_WIDTH,
        );*/
        append_be_fixed_width(
            &mut sig_bits,
            &self.args.token_id.unwrap(),
            franklin_constants::TOKENID_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self.args.fee.unwrap(),
            franklin_constants::FEE_MANTISSA_BIT_WIDTH + franklin_constants::FEE_EXPONENT_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self.from_before.witness.account_witness.nonce.unwrap(),
            franklin_constants::NONCE_BIT_WIDTH,
        );
        sig_bits
    }
}

impl TransferWitness<Bn256> {
    fn apply_data(tree: &mut CircuitAccountTree, transfer: &TransferData) -> Self {
        //preparing data and base witness
        let before_root = tree.root_hash();
        debug!("Initial root = {}", before_root);
        let (audit_path_from_before, audit_token_path_from_before) =
            get_audits(tree, transfer.from_account_address, transfer.token_id);

        let (audit_path_to_before, audit_token_path_to_before) =
            get_audits(tree, transfer.to_account_address, transfer.token_id);

        let capacity = tree.capacity();
        assert_eq!(capacity, 1 << franklin_constants::account_tree_depth());
        let account_address_from_fe =
            Fr::from_str(&transfer.from_account_address.to_string()).unwrap();
        let account_address_to_fe = Fr::from_str(&transfer.to_account_address.to_string()).unwrap();
        let token_id_fe = Fr::from_str(&transfer.token_id.to_string()).unwrap();

        let fee_as_field_element = Fr::from_str(&transfer.fee.to_string()).unwrap();

        let fee_bits = convert_to_float(
            transfer.fee,
            franklin_constants::FEE_EXPONENT_BIT_WIDTH,
            franklin_constants::FEE_MANTISSA_BIT_WIDTH,
            10,
        )
        .unwrap();

        let fee_encoded: Fr = le_bit_vector_into_field_element(&fee_bits);

        //applying first transfer part
        let (
            account_witness_from_before,
            account_witness_from_intermediate,
            balance_from_before,
            balance_from_intermediate,
        ) = apply_leaf_operation(
            tree,
            transfer.from_account_address,
            None,
            Some(transfer.token_id),
            |acc| {
                acc.nonce.add_assign(&Fr::from_str("1").unwrap());
            },
        );

        let intermediate_root = tree.root_hash();
        debug!("Intermediate root = {}", intermediate_root);

        let (audit_path_from_intermediate, audit_token_path_from_intermediate) =
            get_audits(tree, transfer.from_account_address, transfer.token_id);

        let (audit_path_to_intermediate, audit_token_path_to_intermediate) =
            get_audits(tree, transfer.to_account_address, transfer.token_id);

        let (
            account_witness_to_intermediate,
            account_witness_to_after,
            balance_to_intermediate,
            balance_to_after,
        ) = apply_leaf_operation(
            tree,
            transfer.to_account_address,
            Some(transfer.token_id),
            None,
            |_| {},
        );
        let after_root = tree.root_hash();
        let (audit_path_from_after, audit_token_path_from_after) =
            get_audits(tree, transfer.from_account_address, transfer.token_id);

        let (audit_path_to_after, audit_token_path_to_after) =
            get_audits(tree, transfer.to_account_address, transfer.token_id);

        //calculate a and b
        //let a = balance_from_before;
        //let mut b = amount_as_field_element;
        //b.add_assign(&fee_as_field_element);

        TransferWitness {
            from_before: OperationBranch {
                address: Some(account_address_from_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_from_before,
                    account_path: audit_path_from_before,
                    //balance_value: Some(balance_from_before),
                    //balance_subtree_path: audit_balance_path_from_before,
                    token_subtree_path: audit_token_path_from_before,
                },
            },
            from_intermediate: OperationBranch {
                address: Some(account_address_from_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_from_intermediate.clone(),
                    account_path: audit_path_from_intermediate,
                    //balance_value: Some(balance_from_intermediate),
                    //balance_subtree_path: audit_balance_path_from_intermediate,
                    token_subtree_path: audit_token_path_from_intermediate,
                },
            },
            from_after: OperationBranch {
                address: Some(account_address_from_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_from_intermediate,
                    account_path: audit_path_from_after,
                    //balance_value: Some(balance_from_intermediate),
                    //balance_subtree_path: audit_balance_path_from_after,
                    token_subtree_path: audit_token_path_from_after,
                },
            },
            to_before: OperationBranch {
                address: Some(account_address_to_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_to_intermediate.clone(),
                    account_path: audit_path_to_before,
                    //balance_value: Some(balance_to_intermediate),
                    //balance_subtree_path: audit_balance_path_to_before,
                    token_subtree_path: audit_token_path_to_before,
                },
            },
            to_intermediate: OperationBranch {
                address: Some(account_address_to_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_to_intermediate,
                    account_path: audit_path_to_intermediate,
                    //balance_value: Some(balance_to_intermediate),
                    //balance_subtree_path: audit_balance_path_to_intermediate,
                    token_subtree_path: audit_token_path_to_intermediate,
                },
            },
            to_after: OperationBranch {
                address: Some(account_address_to_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_to_after,
                    account_path: audit_path_to_after,
                    //balance_value: Some(balance_to_after),
                    //balance_subtree_path: audit_balance_path_to_after,
                    token_subtree_path: audit_token_path_to_after,
                },
            },
            args: OperationArguments {
                eth_address: Some(Fr::zero()),
                //amount_packed: Some(amount_encoded),
                //full_amount: Some(amount_as_field_element),
                fee: Some(fee_encoded),
                pub_nonce: Some(Fr::zero()),
                //a: Some(a),
                //b: Some(b),
                new_pub_key_hash: Some(Fr::zero()),
                token_id: Some(token_id_fe),
            },
            before_root: Some(before_root),
            intermediate_root: Some(intermediate_root),
            after_root: Some(after_root),
            tx_type: Some(Fr::from_str("5").unwrap()),
        }
    }
}
