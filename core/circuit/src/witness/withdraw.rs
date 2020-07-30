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
        utils::{append_be_fixed_width, eth_address_to_fr, le_bit_vector_into_field_element},
    },
    node::operations::WithdrawOp,
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

pub struct WithdrawData {
    pub fee: u128,
    pub token_id: u32,
    pub account_address: u32,
    pub eth_address: Fr,
}

pub struct WithdrawWitness<E: RescueEngine> {
    pub before: OperationBranch<E>,
    pub after: OperationBranch<E>,
    pub args: OperationArguments<E>,
    pub before_root: Option<E::Fr>,
    pub after_root: Option<E::Fr>,
    pub tx_type: Option<E::Fr>,
}

impl Witness for WithdrawWitness<Bn256> {
    type OperationType = WithdrawOp;
    type CalculateOpsInput = SigDataInput;

    fn apply_tx(tree: &mut CircuitAccountTree, withdraw: &WithdrawOp) -> Self {
        let withdraw_data = WithdrawData {
            fee: big_decimal_to_u128(&withdraw.tx.fee),
            token_id: u32::from(withdraw.tx.token_id),
            account_address: withdraw.account_id,
            eth_address: eth_address_to_fr(&withdraw.tx.to),
        };
        // le_bit_vector_into_field_element()
        Self::apply_data(tree, &withdraw_data)
    }

    fn get_pubdata(&self) -> Vec<bool> {
        let mut pubdata_bits = vec![];
        append_be_fixed_width(
            &mut pubdata_bits,
            &self.tx_type.unwrap(),
            franklin_constants::TX_TYPE_BIT_WIDTH,
        );

        append_be_fixed_width(
            &mut pubdata_bits,
            &self.before.address.unwrap(),
            franklin_constants::ACCOUNT_ID_BIT_WIDTH,
        );
        /*append_be_fixed_width(
            &mut pubdata_bits,
            &self.before.token.unwrap(),
            franklin_constants::TOKEN_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut pubdata_bits,
            &self.args.full_amount.unwrap(),
            franklin_constants::BALANCE_BIT_WIDTH,
        );*/
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

        append_be_fixed_width(
            &mut pubdata_bits,
            &self.args.eth_address.unwrap(),
            franklin_constants::ETH_ADDRESS_BIT_WIDTH,
        );
        pubdata_bits.resize(6 * franklin_constants::CHUNK_BIT_WIDTH, false);
        pubdata_bits
    }

    fn calculate_operations(&self, input: SigDataInput) -> Vec<Operation<Bn256>> {
        let pubdata_chunks: Vec<_> = self
            .get_pubdata()
            .chunks(64)
            .map(|x| le_bit_vector_into_field_element(&x.to_vec()))
            .collect();

        let operation_zero = Operation {
            new_root: self.after_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("0").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[0]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.before.clone(),
            rhs: self.before.clone(),
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
            lhs: self.after.clone(),
            rhs: self.after.clone(),
        };

        let operation_two = Operation {
            new_root: self.after_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("2").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[2]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.after.clone(),
            rhs: self.after.clone(),
        };

        let operation_three = Operation {
            new_root: self.after_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("3").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[3]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.after.clone(),
            rhs: self.after.clone(),
        };
        let operation_four = Operation {
            new_root: self.after_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("4").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[4]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.after.clone(),
            rhs: self.after.clone(),
        };
        let operation_five = Operation {
            new_root: self.after_root,
            tx_type: self.tx_type,
            chunk: Some(Fr::from_str("5").unwrap()),
            pubdata_chunk: Some(pubdata_chunks[5]),
            first_sig_msg: Some(input.first_sig_msg),
            second_sig_msg: Some(input.second_sig_msg),
            third_sig_msg: Some(input.third_sig_msg),
            signature_data: input.signature.clone(),
            signer_pub_key_packed: input.signer_pub_key_packed.to_vec(),
            args: self.args.clone(),
            lhs: self.after.clone(),
            rhs: self.after.clone(),
        };
        vec![
            operation_zero,
            operation_one,
            operation_two,
            operation_three,
            operation_four,
            operation_five,
        ]
    }
}

impl<E: RescueEngine> WithdrawWitness<E> {
    pub fn get_sig_bits(&self) -> Vec<bool> {
        let mut sig_bits = vec![];
        append_be_fixed_width(
            &mut sig_bits,
            &Fr::from_str("3").unwrap(), //Corresponding tx_type
            franklin_constants::TX_TYPE_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self.before.witness.account_witness.pub_key_hash.unwrap(),
            franklin_constants::NEW_PUBKEY_HASH_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self.args.eth_address.unwrap(),
            franklin_constants::ETH_ADDRESS_BIT_WIDTH,
        );
        /*append_be_fixed_width(
            &mut sig_bits,
            &self.before.token.unwrap(),
            franklin_constants::TOKEN_BIT_WIDTH,
        );
        append_be_fixed_width(
            &mut sig_bits,
            &self.args.full_amount.unwrap(),
            franklin_constants::BALANCE_BIT_WIDTH,
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
            &self.before.witness.account_witness.nonce.unwrap(),
            franklin_constants::NONCE_BIT_WIDTH,
        );
        sig_bits
    }
}

impl WithdrawWitness<Bn256> {
    fn apply_data(tree: &mut CircuitAccountTree, withdraw: &WithdrawData) -> Self {
        //preparing data and base witness
        let before_root = tree.root_hash();
        debug!("Initial root = {}", before_root);
        let (audit_path_before, audit_token_path_before) =
            get_audits(tree, withdraw.account_address, withdraw.token_id);

        let capacity = tree.capacity();
        assert_eq!(capacity, 1 << franklin_constants::account_tree_depth());
        let account_address_fe = Fr::from_str(&withdraw.account_address.to_string()).unwrap();
        let token_fe = Fr::from_str(&withdraw.token_id.to_string()).unwrap();

        let fee_as_field_element = Fr::from_str(&withdraw.fee.to_string()).unwrap();

        let fee_bits = convert_to_float(
            withdraw.fee,
            franklin_constants::FEE_EXPONENT_BIT_WIDTH,
            franklin_constants::FEE_MANTISSA_BIT_WIDTH,
            10,
        )
        .unwrap();

        let fee_encoded: Fr = le_bit_vector_into_field_element(&fee_bits);

        //calculate a and b

        //applying withdraw

        let (account_witness_before, account_witness_after, balance_before, balance_after) =
            apply_leaf_operation(
                tree,
                withdraw.account_address,
                None,
                Some(withdraw.token_id),
                |acc| {
                    acc.nonce.add_assign(&Fr::from_str("1").unwrap());
                },
            );

        let after_root = tree.root_hash();
        debug!("After root = {}", after_root);
        let (audit_path_after, audit_token_path_after) =
            get_audits(tree, withdraw.account_address, withdraw.token_id);

        //let a = balance_before;
        //let mut b = amount_as_field_element;
        //b.add_assign(&fee_as_field_element);

        WithdrawWitness {
            before: OperationBranch {
                address: Some(account_address_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_before,
                    account_path: audit_path_before,
                    //balance_value: Some(balance_before),
                    token_subtree_path: audit_token_path_before,
                },
            },
            after: OperationBranch {
                address: Some(account_address_fe),
                witness: OperationBranchWitness {
                    account_witness: account_witness_after,
                    account_path: audit_path_after,
                    //balance_value: Some(balance_after),
                    token_subtree_path: audit_token_path_after,
                },
            },
            args: OperationArguments {
                eth_address: Some(withdraw.eth_address),
                //amount_packed: Some(amount_encoded),
                //full_amount: Some(amount_as_field_element),
                fee: Some(fee_encoded),
                pub_nonce: Some(Fr::zero()),
                //a: Some(a),
                //b: Some(b),
                new_pub_key_hash: Some(Fr::zero()),
                token_id: Some(token_fe),
            },
            before_root: Some(before_root),
            after_root: Some(after_root),
            tx_type: Some(Fr::from_str("3").unwrap()),
        }
    }
}
