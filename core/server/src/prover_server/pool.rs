// Built-in
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::{thread, time};
// External
use crate::franklin_crypto::bellman::pairing::ff::PrimeField;
use futures::channel::mpsc;
use log::info;
// Workspace deps
use circuit::witness::{
    utils::{SigDataInput, WitnessBuilder},
    ChangePubkeyOffChainWitness, CloseAccountWitness, DepositWitness, FullExitWitness,
    TransferToNewWitness, TransferWitness, WithdrawWitness, Witness,
};
use models::{
    circuit::CircuitAccountTree,
    config_options::ThreadPanicNotify,
    node::{BlockNumber, Fr, FranklinOp},
    Operation,
};
use plasma::state::CollectedFee;
use prover::prover_data::ProverData;

#[derive(Debug, Clone)]
struct OperationsQueue {
    operations: VecDeque<(Operation, bool)>,
    last_loaded_block: BlockNumber,
}

impl OperationsQueue {
    fn new(last_loaded_block: BlockNumber) -> Self {
        Self {
            operations: VecDeque::new(),
            last_loaded_block,
        }
    }

    /// Fills the operations queue if the amount of non-processed operations
    /// is less than `limit`.
    fn take_next_commits_if_needed(
        &mut self,
        conn_pool: &storage::ConnectionPool,
        limit: i64,
    ) -> Result<(), String> {
        if self.operations.len() < limit as usize {
            let storage = conn_pool.access_storage().expect("failed to connect to db");
            let ops = storage
                .chain()
                .block_schema()
                .load_commits_after_block(self.last_loaded_block, limit)
                .map_err(|e| format!("failed to read commit operations: {}", e))?;

            self.operations.extend(ops);

            if let Some((op, _)) = self.operations.back() {
                self.last_loaded_block = op.block.block_number;
            }

            trace!(
                "Operations: {:?}",
                self.operations
                    .iter()
                    .map(|(op, _)| op.block.block_number)
                    .collect::<Vec<_>>()
            );
        }

        Ok(())
    }

    /// Takes the oldest non-processed operation out of the queue and whether it has a proof or not.
    /// Returns `None` if there are no non-processed operations.
    fn take_next_operation(&mut self) -> Option<(Operation, bool)> {
        self.operations.pop_front()
    }

    /// Return block number of the next operation to take.
    fn next_block_number(&self) -> Option<u32> {
        if self.operations.is_empty() {
            None
        } else {
            Some(self.operations[0].0.block.block_number)
        }
    }

    // Whether queue is empty or not.
    fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

pub struct ProversDataPool {
    limit: i64,
    op_queue: OperationsQueue,
    prepared: HashMap<BlockNumber, ProverData>,
}

impl ProversDataPool {
    pub fn new(last_loaded_block: BlockNumber, limit: i64) -> Self {
        Self {
            limit,
            op_queue: OperationsQueue::new(last_loaded_block),
            prepared: HashMap::new(),
        }
    }

    pub fn get(&self, block: BlockNumber) -> Option<&ProverData> {
        self.prepared.get(&block)
    }

    pub fn clean_up(&mut self, block: BlockNumber) {
        self.prepared.remove(&block);
    }
}

/// `Maintainer` is a helper structure that maintains the
/// prover data pool.
///
/// The essential part of this structure is `maintain` function
/// which runs forever and adds data to the externally owned
/// pool. It maintains its own circuit tree to generates witness.
/// Initial circuit tree is substituted in constructor.
///
/// `migrate` function is private and is invoked by the
/// public `start` function, which starts
/// the named thread dedicated for that routine only.
pub struct Maintainer {
    /// Connection to the database.
    conn_pool: storage::ConnectionPool,
    /// Thread-safe reference to the data pool.
    data: Arc<RwLock<ProversDataPool>>,
    /// Routine refresh interval.
    rounds_interval: time::Duration,
    /// Cached account state.
    ///
    /// This field is initialized at the first iteration of `maintain`
    /// routine, and is updated by applying the state diff after that.
    account_tree: CircuitAccountTree,
    /// Maintainer prepared prover for blocks in order
    /// `next_block_number` stores next block number to prepare data for.
    next_block_number: BlockNumber,
}

impl Maintainer {
    /// Creates a new `Maintainer` object.
    pub fn new(
        conn_pool: storage::ConnectionPool,
        data: Arc<RwLock<ProversDataPool>>,
        rounds_interval: time::Duration,
        account_tree: CircuitAccountTree,
        block_number: BlockNumber,
    ) -> Self {
        Self {
            conn_pool,
            data,
            rounds_interval,
            account_tree,
            next_block_number: block_number + 1,
        }
    }

    /// Starts the thread running `maintain` method.
    pub fn start(mut self, panic_notify: mpsc::Sender<bool>) {
        thread::Builder::new()
            .name("prover_server_pool".to_string())
            .spawn(move || {
                let _panic_sentinel = ThreadPanicNotify(panic_notify);
                self.maintain();
            })
            .expect("failed to start provers server");
    }

    /// Updates the pool data in an infinite loop, awaiting `rounds_interval` time
    /// between updates.
    fn maintain(&mut self) {
        info!("preparing prover data routine started");
        loop {
            self.take_next_commits_if_needed()
                .expect("couldn't get next commits");
            self.prepare_next().expect("couldn't prepare next commits");
            thread::sleep(self.rounds_interval);
        }
    }

    /// Loads the operations to process for every available prover queue.
    fn take_next_commits_if_needed(&mut self) -> Result<(), String> {
        // When updating this method, be sure to not hold lock longer than necessary,
        // since it can cause provers to not be able to interact with the server.

        // Clone the required data to process it without holding the lock.
        let (mut queue, limit) = {
            let pool = self.data.read().expect("failed to get write lock on data");
            (pool.op_queue.clone(), pool.limit)
        };

        // Process queue and fill it with data.
        queue.take_next_commits_if_needed(&self.conn_pool, limit)?;

        // Update the queues in pool.
        // Since this structure is the only writer to the queues, it is guaranteed
        // to not contain data that will be overwritten by the assignment.
        let mut pool = self.data.write().expect("failed to get write lock on data");
        pool.op_queue = queue;

        Ok(())
    }

    /// Goes through existing queues of operations and builds a prover data for each of them.
    fn prepare_next(&mut self) -> Result<(), String> {
        // When updating this method, be sure to not hold lock longer than necessary,
        // since it can cause provers to not be able to interact with the server.

        // Clone the queues to process them without holding the lock.
        let mut queue = {
            let pool = self.data.read().expect("failed to get write lock on data");

            pool.op_queue.clone()
        };

        // Create a storage for prepared data.
        let mut prepared = HashMap::new();

        // Go through queue, take the next operation to process, and build the
        // prover data for it.
        while !queue.is_empty() {
            assert_eq!(
                Some(self.next_block_number),
                queue.next_block_number(),
                "Blocks must be processed in order"
            );
            if let Some((op, has_proof)) = queue.take_next_operation() {
                let storage = self
                    .conn_pool
                    .access_storage()
                    .expect("failed to connect to db");
                let pd = self.build_prover_data(&storage, &op)?;
                // Always build prover data to update circuit tree to the next block, but store only
                // if there is no proof for the block.
                if !has_proof {
                    prepared.insert(op.block.block_number, pd);
                }
                self.next_block_number = op.block.block_number + 1;
            }
        }

        // Update the queue and prepared data in pool.
        // Since this structure is the only writer to the queue, it is guaranteed
        // to not contain data that will be overwritten by the assignment.
        // Prepared data is appended to the existing one, thus we can not worry about
        // synchronization as well.
        let mut pool = self.data.write().expect("failed to get write lock on data");
        pool.op_queue = queue;
        pool.prepared.extend(prepared);

        Ok(())
    }

    fn build_prover_data(
        &mut self,
        storage: &storage::StorageProcessor,
        commit_operation: &models::Operation,
    ) -> Result<ProverData, String> {
        let block_number = commit_operation.block.block_number;
        let block_size = commit_operation.block.block_chunks_size;

        info!("building prover data for block {}", &block_number);

        let mut witness_accum = WitnessBuilder::new(
            &mut self.account_tree,
            commit_operation.block.fee_account,
            block_number,
        );

        let ops = storage
            .chain()
            .block_schema()
            .get_block_operations(block_number)
            .map_err(|e| format!("failed to get block operations {}", e))?;

        let mut operations = vec![];
        let mut pub_data = vec![];
        let mut fees = vec![];
        for op in ops {
            match op {
                FranklinOp::Deposit(deposit) => {
                    let deposit_witness =
                        DepositWitness::apply_tx(&mut witness_accum.account_tree, &deposit);

                    let deposit_operations = deposit_witness.calculate_operations(());
                    operations.extend(deposit_operations);
                    pub_data.extend(deposit_witness.get_pubdata());
                }
                FranklinOp::Transfer(transfer) => {
                    let transfer_witness =
                        TransferWitness::apply_tx(&mut witness_accum.account_tree, &transfer);

                    let input = SigDataInput::from_transfer_op(&transfer)?;
                    let transfer_operations = transfer_witness.calculate_operations(input);

                    operations.extend(transfer_operations);
                    fees.push(CollectedFee {
                        token: 0, // TODO ADE must be removed
                        amount: transfer.tx.fee,
                    });
                    pub_data.extend(transfer_witness.get_pubdata());
                }
                FranklinOp::TransferToNew(transfer_to_new) => {
                    let transfer_to_new_witness = TransferToNewWitness::apply_tx(
                        &mut witness_accum.account_tree,
                        &transfer_to_new,
                    );

                    let input = SigDataInput::from_transfer_to_new_op(&transfer_to_new)?;
                    let transfer_to_new_operations =
                        transfer_to_new_witness.calculate_operations(input);

                    operations.extend(transfer_to_new_operations);
                    fees.push(CollectedFee {
                        token: 0, // TODO ADE must be removed
                        amount: transfer_to_new.tx.fee,
                    });
                    pub_data.extend(transfer_to_new_witness.get_pubdata());
                }
                FranklinOp::Withdraw(withdraw) => {
                    let withdraw_witness =
                        WithdrawWitness::apply_tx(&mut witness_accum.account_tree, &withdraw);

                    let input = SigDataInput::from_withdraw_op(&withdraw)?;
                    let withdraw_operations = withdraw_witness.calculate_operations(input);

                    operations.extend(withdraw_operations);
                    fees.push(CollectedFee {
                        token: 0, // TODO ADE must be removed
                        amount: withdraw.tx.fee,
                    });
                    pub_data.extend(withdraw_witness.get_pubdata());
                }
                FranklinOp::Close(close) => {
                    let close_account_witness =
                        CloseAccountWitness::apply_tx(&mut witness_accum.account_tree, &close);

                    let input = SigDataInput::from_close_op(&close)?;
                    let close_account_operations =
                        close_account_witness.calculate_operations(input);

                    operations.extend(close_account_operations);
                    pub_data.extend(close_account_witness.get_pubdata());
                }
                FranklinOp::FullExit(full_exit_op) => {
                    /*let success = full_exit_op.withdraw_amount.is_some();

                    let full_exit_witness = FullExitWitness::apply_tx(
                        &mut witness_accum.account_tree,
                        &(*full_exit_op, success),
                    );

                    let full_exit_operations = full_exit_witness.calculate_operations(());

                    operations.extend(full_exit_operations);
                    pub_data.extend(full_exit_witness.get_pubdata());*/
                    // TODO ADE full exit not managed for now
                    unimplemented!()
                }
                FranklinOp::ChangePubKeyOffchain(change_pkhash_op) => {
                    let change_pkhash_witness = ChangePubkeyOffChainWitness::apply_tx(
                        &mut witness_accum.account_tree,
                        &change_pkhash_op,
                    );

                    let change_pkhash_operations = change_pkhash_witness.calculate_operations(());

                    operations.extend(change_pkhash_operations);
                    pub_data.extend(change_pkhash_witness.get_pubdata());
                }
                FranklinOp::Noop(_) => {} // Noops are handled below
            }
        }

        witness_accum.add_operation_with_pubdata(operations, pub_data);
        witness_accum.extend_pubdata_with_noops(block_size);
        assert_eq!(witness_accum.pubdata.len(), 64 * block_size);
        assert_eq!(witness_accum.operations.len(), block_size);

        witness_accum.collect_fees(&fees);
        assert_eq!(
            witness_accum
                .root_after_fees
                .expect("root_after_fees not present"),
            commit_operation.block.new_root_hash
        );
        witness_accum.calculate_pubdata_commitment();

        Ok(ProverData {
            public_data_commitment: witness_accum.pubdata_commitment.unwrap(),
            old_root: witness_accum.initial_root_hash,
            new_root: commit_operation.block.new_root_hash,
            validator_address: Fr::from_str(&commit_operation.block.fee_account.to_string())
                .expect("failed to parse"),
            operations: witness_accum.operations,
            validator_balances: vec![], //witness_accum.fee_account_balances.unwrap(),
            validator_audit_path: vec![], //witness_accum.fee_account_audit_path.unwrap(),
            validator_account: witness_accum.fee_account_witness.unwrap(),
        })
    }
}
