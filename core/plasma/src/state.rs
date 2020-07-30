use bigdecimal::BigDecimal;
use failure::{bail, ensure, format_err, Error};
use log::trace;
use models::node::operations::{
    ChangePubKeyOp, CloseOp, DepositOp, FranklinOp, FullExitOp, TransferOp, TransferToNewOp,
    WithdrawOp,
};
use models::node::tx::ChangePubKey;
use models::node::Address;
use models::node::{Account, AccountTree, FranklinPriorityOp, PubKeyHash};
use models::node::{
    AccountId, AccountMap, AccountUpdate, AccountUpdates, BlockNumber, Fr, TokenId,
};
use models::node::{Close, Deposit, FranklinTx, FullExit, Transfer, Withdraw};
use models::params;
use std::collections::HashMap;

#[derive(Debug)]
pub struct OpSuccess {
    pub fee: Option<CollectedFee>,
    pub updates: AccountUpdates,
    pub executed_op: FranklinOp,
}

#[derive(Debug, Clone)]
pub struct PlasmaState {
    /// Accounts stored in a sparse Merkle tree
    token_tree: AccountTree,

    account_id_by_address: HashMap<Address, AccountId>,

    /// Current block number
    pub block_number: BlockNumber,
}

// TODO ADE: must be removed
#[derive(Debug, Clone)]
pub struct CollectedFee {
    pub token: TokenId,
    pub amount: BigDecimal,
}

impl PlasmaState {
    pub fn empty() -> Self {
        let tree_depth = params::account_tree_depth();
        let token_tree = AccountTree::new(tree_depth);
        Self {
            token_tree,
            block_number: 0,
            account_id_by_address: HashMap::new(),
        }
    }

    pub fn from_acc_map(accounts: AccountMap, current_block: BlockNumber) -> Self {
        let mut empty = Self::empty();
        empty.block_number = current_block;
        for (id, account) in accounts {
            empty.insert_account(id, account);
        }
        empty
    }

    pub fn new(
        token_tree: AccountTree,
        account_id_by_address: HashMap<Address, AccountId>,
        current_block: BlockNumber,
    ) -> Self {
        Self {
            token_tree,
            block_number: current_block,
            account_id_by_address,
        }
    }

    pub fn get_accounts(&self) -> Vec<(u32, Account)> {
        self.token_tree
            .items
            .iter()
            .map(|a| (*a.0 as u32, a.1.clone()))
            .collect()
    }

    pub fn root_hash(&self) -> Fr {
        self.token_tree.root_hash()
    }

    pub fn get_account(&self, account_id: AccountId) -> Option<Account> {
        self.token_tree.get(account_id).cloned()
    }

    pub fn chunks_for_tx(&self, franklin_tx: &FranklinTx) -> usize {
        match franklin_tx {
            FranklinTx::Transfer(tx) => {
                if self.get_account_by_address(&tx.to).is_some() {
                    TransferOp::CHUNKS
                } else {
                    TransferToNewOp::CHUNKS
                }
            }
            _ => franklin_tx.min_chunks(),
        }
    }

    /// Priority op execution should not fail.
    pub fn execute_priority_op(&mut self, op: FranklinPriorityOp) -> OpSuccess {
        match op {
            FranklinPriorityOp::Deposit(op) => self.apply_deposit(op),
            FranklinPriorityOp::FullExit(op) => self.apply_full_exit(op),
        }
    }

    pub fn execute_tx(&mut self, tx: FranklinTx) -> Result<OpSuccess, Error> {
        match tx {
            FranklinTx::Transfer(tx) => self.apply_transfer(*tx),
            FranklinTx::Withdraw(tx) => self.apply_withdraw(*tx),
            FranklinTx::Close(tx) => self.apply_close(*tx),
            FranklinTx::ChangePubKey(tx) => self.apply_change_pubkey(*tx),
        }
    }

    fn get_free_account_id(&self) -> AccountId {
        // TODO check for collisions.
        self.token_tree.items.len() as u32
    }

    fn apply_deposit(&mut self, priority_op: Deposit) -> OpSuccess {
        let account_id = if let Some((account_id, _)) = self.get_account_by_address(&priority_op.to)
        {
            account_id
        } else {
            self.get_free_account_id()
        };
        let deposit_op = DepositOp {
            priority_op,
            account_id,
        };

        let updates = self.apply_deposit_op(&deposit_op);
        OpSuccess {
            fee: None,
            updates,
            executed_op: FranklinOp::Deposit(Box::new(deposit_op)),
        }
    }

    fn apply_full_exit(&mut self, priority_op: FullExit) -> OpSuccess {
        // NOTE: Authroization of the FullExit is verified on the contract.
        // TODO ADE: with balances, full exit returns the total amount of the token. check what to do in case of 721
        trace!("Processing {:?}", priority_op);
        let op = FullExitOp { priority_op };

        OpSuccess {
            fee: None,
            updates: self.apply_full_exit_op(&op),
            executed_op: FranklinOp::FullExit(Box::new(op)),
        }
    }

    pub fn apply_full_exit_op(&mut self, op: &FullExitOp) -> AccountUpdates {
        let mut updates = Vec::new();
        let account_id = op.priority_op.account_id;

        // expect is ok since account since existence was verified before
        // TODO ADE: I think we need to put token list in FullExitOp, for verification only
        let mut account = self
            .get_account(account_id)
            .expect("Full exit account not found");

        let old_nonce = account.nonce;

        let tokens = account.get_tokens();
        for token in tokens {
            account.remove_token(token);
            updates.push((
                account_id,
                AccountUpdate::RemoveToken {
                    new_nonce: old_nonce,
                    old_nonce,
                    token,
                },
            ))
        }
        self.insert_account(account_id, account);

        updates
    }

    fn apply_transfer(&mut self, tx: Transfer) -> Result<OpSuccess, Error> {
        ensure!(
            tx.token_id < (params::total_tokens() as TokenId),
            "Token id is not supported"
        );
        let (from, from_account) = self
            .get_account_by_address(&tx.from)
            .ok_or_else(|| format_err!("From account does not exist"))?;
        ensure!(
            from_account.pub_key_hash != PubKeyHash::default(),
            "Account is locked"
        );
        ensure!(
            tx.verify_signature() == Some(from_account.pub_key_hash),
            "Transfer signature is incorrect"
        );
        ensure!(from == tx.account_id, "Transfer account id is incorrect");

        if let Some((to, _)) = self.get_account_by_address(&tx.to) {
            let transfer_op = TransferOp { tx, from, to };

            let (fee, updates) = self.apply_transfer_op(&transfer_op)?;
            Ok(OpSuccess {
                fee: Some(fee),
                updates,
                executed_op: FranklinOp::Transfer(Box::new(transfer_op)),
            })
        } else {
            let to = self.get_free_account_id();
            let transfer_to_new_op = TransferToNewOp { tx, from, to };

            let (fee, updates) = self.apply_transfer_to_new_op(&transfer_to_new_op)?;
            Ok(OpSuccess {
                fee: Some(fee),
                updates,
                executed_op: FranklinOp::TransferToNew(Box::new(transfer_to_new_op)),
            })
        }
    }

    fn apply_withdraw(&mut self, tx: Withdraw) -> Result<OpSuccess, Error> {
        ensure!(
            tx.token_id < (params::total_tokens() as TokenId),
            "Token id is not supported"
        );
        let (account_id, account) = self
            .get_account_by_address(&tx.from)
            .ok_or_else(|| format_err!("Account does not exist"))?;
        ensure!(
            account.pub_key_hash != PubKeyHash::default(),
            "Account is locked"
        );
        ensure!(
            tx.verify_signature() == Some(account.pub_key_hash),
            "withdraw signature is incorrect"
        );
        ensure!(
            account_id == tx.account_id,
            "Withdraw account id is incorrect"
        );
        let withdraw_op = WithdrawOp { tx, account_id };

        let (fee, updates) = self.apply_withdraw_op(&withdraw_op)?;
        Ok(OpSuccess {
            fee: Some(fee),
            updates,
            executed_op: FranklinOp::Withdraw(Box::new(withdraw_op)),
        })
    }

    fn apply_close(&mut self, _tx: Close) -> Result<OpSuccess, Error> {
        bail!("Account closing is disabled");
        // let (account_id, account) = self
        //     .get_account_by_address(&tx.account)
        //     .ok_or_else(|| format_err!("Account does not exist"))?;
        // let close_op = CloseOp { tx, account_id };
        //        ensure!(account.pub_key_hash != PubKeyHash::default(), "Account is locked");
        // ensure!(
        //     tx.verify_signature() == Some(account.pub_key_hash),
        //     "withdraw signature is incorrect"
        // );

        // let (fee, updates) = self.apply_close_op(&close_op)?;
        // Ok(OpSuccess {
        //     fee: Some(fee),
        //     updates,
        //     executed_op: FranklinOp::Close(Box::new(close_op)),
        // })
    }

    fn apply_change_pubkey(&mut self, tx: ChangePubKey) -> Result<OpSuccess, Error> {
        let (account_id, account) = self
            .get_account_by_address(&tx.account)
            .ok_or_else(|| format_err!("Account does not exist"))?;
        ensure!(
            tx.eth_signature.is_none() || tx.verify_eth_signature() == Some(account.address),
            "ChangePubKey signature is incorrect"
        );
        ensure!(
            account_id == tx.account_id,
            "ChangePubKey account id is incorrect"
        );
        let change_pk_op = ChangePubKeyOp { tx, account_id };

        let (fee, updates) = self.apply_change_pubkey_op(&change_pk_op)?;
        Ok(OpSuccess {
            fee: Some(fee),
            updates,
            executed_op: FranklinOp::ChangePubKeyOffchain(Box::new(change_pk_op)),
        })
    }

    pub fn collect_fee(&mut self, fees: &[CollectedFee], fee_account: AccountId) -> AccountUpdates {
        let mut updates = Vec::new();

        let mut account = self.get_account(fee_account).unwrap_or_else(|| {
            panic!(
                "Fee account should be present in the account tree: {}",
                fee_account
            )
        });

        // TODO ADE: fees are (currently) disabled
        /*
        for fee in fees {
            if fee.amount == BigDecimal::from(0) {
                continue;
            }

            let old_amount = account.get_balance(fee.token).clone();
            let nonce = account.nonce;
            account.add_balance(fee.token, &fee.amount);
            let new_amount = account.get_balance(fee.token).clone();

            updates.push((
                fee_account,
                AccountUpdate::UpdateBalance {
                    balance_update: (fee.token, old_amount, new_amount),
                    old_nonce: nonce,
                    new_nonce: nonce,
                },
            ));
        }

        self.insert_account(fee_account, account);
        */

        updates
    }

    pub fn get_account_by_address(&self, address: &Address) -> Option<(AccountId, Account)> {
        let account_id = *self.account_id_by_address.get(address)?;
        Some((
            account_id,
            self.get_account(account_id)
                .expect("Failed to get account by cached pubkey"),
        ))
    }

    #[doc(hidden)] // Public for benches.
    pub fn insert_account(&mut self, id: AccountId, account: Account) {
        self.account_id_by_address
            .insert(account.address.clone(), id);
        self.token_tree.insert(id, account);
    }

    #[allow(dead_code)]
    fn remove_account(&mut self, id: AccountId) {
        if let Some(account) = self.get_account(id) {
            self.account_id_by_address.remove(&account.address);
            self.token_tree.remove(id);
        }
    }

    pub fn apply_deposit_op(&mut self, op: &DepositOp) -> AccountUpdates {
        let mut updates = Vec::new();

        let mut account = self.get_account(op.account_id).unwrap_or_else(|| {
            let (account, upd) = Account::create_account(op.account_id, op.priority_op.to);
            updates.extend(upd.into_iter());
            account
        });

        let old_nonce = account.nonce;
        account.add_token(op.priority_op.token_id);

        self.insert_account(op.account_id, account);

        updates.push((
            op.account_id,
            AccountUpdate::AddToken {
                old_nonce,
                new_nonce: old_nonce,
                token: op.priority_op.token_id,
            },
        ));

        updates
    }

    pub fn apply_transfer_to_new_op(
        &mut self,
        op: &TransferToNewOp,
    ) -> Result<(CollectedFee, AccountUpdates), Error> {
        let mut updates = Vec::new();

        assert!(
            self.get_account(op.to).is_none(),
            "Transfer to new account exists"
        );
        let mut to_account = {
            let (acc, upd) = Account::create_account(op.to, op.tx.to);
            updates.extend(upd.into_iter());
            acc
        };

        let mut from_account = self.get_account(op.from).unwrap();
        let from_old_nonce = from_account.nonce;
        ensure!(op.tx.nonce == from_old_nonce, "Nonce mismatch");
        ensure!(from_account.has_token(op.tx.token_id), "Not current owner");
        from_account.remove_token(op.tx.token_id);
        from_account.nonce += 1;
        let from_new_nonce = from_account.nonce;
        let to_account_nonce = to_account.nonce;
        to_account.add_token(op.tx.token_id);

        self.insert_account(op.from, from_account);
        self.insert_account(op.to, to_account);

        updates.push((
            op.from,
            AccountUpdate::RemoveToken {
                token: op.tx.token_id,
                old_nonce: from_old_nonce,
                new_nonce: from_new_nonce,
            },
        ));
        updates.push((
            op.to,
            AccountUpdate::AddToken {
                token: op.tx.token_id,
                old_nonce: to_account_nonce,
                new_nonce: to_account_nonce,
            },
        ));

        // TODO ADE: remove fees
        let fee = CollectedFee {
            token: op.tx.token_id,
            amount: op.tx.fee.clone(),
        };

        Ok((fee, updates))
    }

    pub fn apply_withdraw_op(
        &mut self,
        op: &WithdrawOp,
    ) -> Result<(CollectedFee, AccountUpdates), Error> {
        let mut updates = Vec::new();
        let mut from_account = self.get_account(op.account_id).unwrap();

        let from_old_nonce = from_account.nonce;

        ensure!(op.tx.nonce == from_old_nonce, "Nonce mismatch");
        ensure!(from_account.has_token(op.tx.token_id), "Not current owner");

        from_account.remove_token(op.tx.token_id);
        from_account.nonce += 1;

        let from_new_nonce = from_account.nonce;

        self.insert_account(op.account_id, from_account);

        updates.push((
            op.account_id,
            AccountUpdate::RemoveToken {
                token: op.tx.token_id,
                old_nonce: from_old_nonce,
                new_nonce: from_new_nonce,
            },
        ));

        // TODO ADE: remove fees
        let fee = CollectedFee {
            token: op.tx.token_id,
            amount: op.tx.fee.clone(),
        };

        Ok((fee, updates))
    }

    pub fn apply_close_op(
        &mut self,
        op: &CloseOp,
    ) -> Result<(CollectedFee, AccountUpdates), Error> {
        let mut updates = Vec::new();
        let account = self.get_account(op.account_id).unwrap();

        for token in 0..params::total_tokens() {
            if !account.get_tokens().is_empty() {
                bail!("Account is not empty");
            }
        }

        ensure!(op.tx.nonce == account.nonce, "Nonce mismatch");

        self.remove_account(op.account_id);

        updates.push((
            op.account_id,
            AccountUpdate::Delete {
                address: account.address,
                nonce: account.nonce,
            },
        ));

        let fee = CollectedFee {
            token: params::ETH_TOKEN_ID,
            amount: BigDecimal::from(0),
        };

        Ok((fee, updates))
    }

    pub fn apply_change_pubkey_op(
        &mut self,
        op: &ChangePubKeyOp,
    ) -> Result<(CollectedFee, AccountUpdates), Error> {
        let mut updates = Vec::new();
        let mut account = self.get_account(op.account_id).unwrap();

        let old_pub_key_hash = account.pub_key_hash.clone();
        let old_nonce = account.nonce;

        ensure!(op.tx.nonce == account.nonce, "Nonce mismatch");
        account.pub_key_hash = op.tx.new_pk_hash.clone();
        account.nonce += 1;

        let new_pub_key_hash = account.pub_key_hash.clone();
        let new_nonce = account.nonce;

        self.insert_account(op.account_id, account);

        updates.push((
            op.account_id,
            AccountUpdate::ChangePubKeyHash {
                old_pub_key_hash,
                old_nonce,
                new_pub_key_hash,
                new_nonce,
            },
        ));

        let fee = CollectedFee {
            token: params::ETH_TOKEN_ID,
            amount: BigDecimal::from(0),
        };

        Ok((fee, updates))
    }

    pub fn apply_transfer_op(
        &mut self,
        op: &TransferOp,
    ) -> Result<(CollectedFee, AccountUpdates), Error> {
        if op.from == op.to {
            return self.apply_transfer_op_to_self(op);
        }

        let mut updates = Vec::new();
        let mut from_account = self.get_account(op.from).unwrap();
        let mut to_account = self.get_account(op.to).unwrap();

        let from_old_nonce = from_account.nonce;

        ensure!(op.tx.nonce == from_old_nonce, "Nonce mismatch");
        ensure!(from_account.has_token(op.tx.token_id), "Not current owner");

        from_account.remove_token(op.tx.token_id);
        from_account.nonce += 1;

        let from_new_nonce = from_account.nonce;

        let to_account_nonce = to_account.nonce;

        to_account.add_token(op.tx.token_id);

        self.insert_account(op.from, from_account);
        self.insert_account(op.to, to_account);

        updates.push((
            op.from,
            AccountUpdate::RemoveToken {
                token: op.tx.token_id,
                old_nonce: from_old_nonce,
                new_nonce: from_new_nonce,
            },
        ));

        updates.push((
            op.to,
            AccountUpdate::AddToken {
                token: op.tx.token_id,
                old_nonce: to_account_nonce,
                new_nonce: to_account_nonce,
            },
        ));

        // TODO ADE: remove fees
        let fee = CollectedFee {
            token: op.tx.token_id,
            amount: op.tx.fee.clone(),
        };

        Ok((fee, updates))
    }

    fn apply_transfer_op_to_self(
        &mut self,
        op: &TransferOp,
    ) -> Result<(CollectedFee, AccountUpdates), Error> {
        // TODO ADE: only fees (+ nonce inc) are collected during this call, this will be removed
        ensure!(
            op.from == op.to,
            "Bug: transfer to self should not be called."
        );

        let mut account = self.get_account(op.from).unwrap();

        let old_nonce = account.nonce;

        ensure!(op.tx.nonce == old_nonce, "Nonce mismatch");
        ensure!(account.has_token(op.tx.token_id), "Not current owner");

        account.nonce += 1;

        let new_nonce = account.nonce;

        self.insert_account(op.from, account);

        let fee = CollectedFee {
            token: op.tx.token_id,
            amount: op.tx.fee.clone(),
        };

        Ok((fee, Vec::new()))
    }
}
