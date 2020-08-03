use std::collections::{BTreeSet, HashMap};
// External uses
use bigdecimal::BigDecimal;
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt, TryFutureExt,
};
use jsonrpc_core::{Error, ErrorCode, IoHandler, MetaIoHandler, Metadata, Middleware, Result};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
// Workspace uses
use models::{
    config_options::{ConfigurationOptions, ThreadPanicNotify},
    node::{
        closest_packable_fee_amount,
        tx::{TxEthSignature, TxHash},
        Account, AccountId, Address, FranklinPriorityOp, FranklinTx, Nonce, PriorityOp, PubKeyHash,
        Token, TokenId, TokenLike,
    },
    primitives::{big_decimal_to_u128, floor_big_decimal, u128_to_bigdecimal},
};
use storage::{
    chain::{
        block::records::BlockDetails, operations::records::StoredExecutedPriorityOperation,
        operations_ext::records::TxReceiptResponse,
    },
    ConnectionPool, StorageProcessor,
};
// Local uses
use crate::{
    eth_watch::EthWatchRequest,
    mempool::{MempoolRequest, TxAddError},
    signature_checker::{VerifiedTx, VerifyTxSignatureRequest},
    state_keeper::StateKeeperRequest,
    utils::shared_lru_cache::SharedLruCache,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponseAccountState {
    //pub balances: HashMap<String, BigDecimal>,
    pub tokens: BTreeSet<u16>,
    pub nonce: Nonce,
    pub pub_key_hash: PubKeyHash,
}

impl ResponseAccountState {
    pub fn try_to_restore(account: Account) -> Result<Self> {
        /*let mut balances = HashMap::new();
        for (token_id, balance) in account.get_nonzero_balances() {
            if token_id == 0 {
                balances.insert("ETH".to_string(), balance);
            } else {
                let token = tokens.get(&token_id).ok_or_else(Error::internal_error)?;
                balances.insert(token.symbol.clone(), balance);
            }
        }*/
        let mut tokens = BTreeSet::new();
        for token in account.get_tokens() {
            tokens.insert(token);
        }

        Ok(Self {
            tokens,
            nonce: account.nonce,
            pub_key_hash: account.pub_key_hash,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DepositingToken {
    token_id: u16,
    expected_accept_block: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DepositingAccountTokens {
    tokens: Vec<DepositingToken>,
}

impl DepositingAccountTokens {
    pub fn from_pending_ops(pending_ops: OngoingDepositsResp) -> Result<Self> {
        let mut tokens = Vec::new();

        for op in pending_ops.deposits {
            let expected_accept_block =
                op.received_on_block + pending_ops.confirmations_for_eth_event;

            //tokens.insert(op.token_id);
            let mut depositing_token = DepositingToken::default();
            depositing_token.token_id = op.token_id;

            // `depositing_token.expected_accept_block` should be the greatest block number among
            // all the deposits for a certain token.
            if expected_accept_block > depositing_token.expected_accept_block {
                depositing_token.expected_accept_block = expected_accept_block;
            }

            tokens.push(depositing_token);
        }

        Ok(Self { tokens })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfoResp {
    pub address: Address,
    pub id: Option<AccountId>,
    depositing: DepositingAccountTokens,
    pub committed: ResponseAccountState,
    pub verified: ResponseAccountState,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub block_number: i64,
    pub committed: bool,
    pub verified: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransactionInfoResp {
    pub executed: bool,
    pub success: Option<bool>,
    pub fail_reason: Option<String>,
    pub block: Option<BlockInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ETHOpInfoResp {
    pub executed: bool,
    pub block: Option<BlockInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContractAddressResp {
    pub main_contract: String,
    pub gov_contract: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferHistory {
    pub from: String,
    pub to: String,
    pub verified: bool,
    pub block_number: i64,
    pub date: String,
    pub tx_hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfoResp {
    pub history: Vec<TransferHistory>,
    pub owner: Option<String>,
}

/// Flattened `PriorityOp` object representing a deposit operation.
/// Used in the `OngoingDepositsResp`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OngoingDeposit {
    received_on_block: u64,
    token_id: u16,
    eth_tx_hash: String,
}

impl OngoingDeposit {
    pub fn new(received_on_block: u64, priority_op: PriorityOp) -> Self {
        let token_id = match priority_op.data {
            FranklinPriorityOp::Deposit(deposit) => deposit.token_id,
            other => {
                panic!("Incorrect input for OngoingDeposit: {:?}", other);
            }
        };

        let eth_tx_hash = hex::encode(&priority_op.eth_hash);

        Self {
            received_on_block,
            token_id,
            eth_tx_hash,
        }
    }
}

/// Information about ongoing deposits for certain recipient address.
///
/// Please note that since this response is based on the events that are
/// currently awaiting confirmations, this information is approximate:
/// blocks on Ethereum can be reverted, and final list of executed deposits
/// can differ from the this estimation.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OngoingDepositsResp {
    /// Address for which response is served.
    address: Address,
    /// List of tuples (Eth block number, Deposit operation) of ongoing
    /// deposit operations.
    deposits: Vec<OngoingDeposit>,

    /// Amount of confirmations required for every deposit to be processed.
    confirmations_for_eth_event: u64,

    /// Estimated block number for deposits completions:
    /// all the deposit operations for provided address are expected to be
    /// accepted in the zkSync network upon reaching this blocks.
    ///
    /// Can be `None` if there are no ongoing deposits.
    estimated_deposits_approval_block: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TxFeeTypes {
    Withdraw,
    Transfer,
}

#[derive(Debug)]
pub enum RpcErrorCodes {
    NonceMismatch = 101,
    IncorrectTx = 103,

    MissingEthSignature = 200,
    EIP1271SignatureVerificationFail = 201,
    IncorrectEthSignature = 202,
    ChangePkNotAuthorized = 203,

    Other = 300,
    AccountCloseDisabled = 301,
}

impl From<TxAddError> for RpcErrorCodes {
    fn from(error: TxAddError) -> Self {
        match error {
            TxAddError::NonceMismatch => Self::NonceMismatch,
            TxAddError::IncorrectTx => Self::IncorrectTx,
            TxAddError::MissingEthSignature => Self::MissingEthSignature,
            TxAddError::EIP1271SignatureVerificationFail => Self::EIP1271SignatureVerificationFail,
            TxAddError::IncorrectEthSignature => Self::IncorrectEthSignature,
            TxAddError::ChangePkNotAuthorized => Self::ChangePkNotAuthorized,
            TxAddError::Other => Self::Other,
        }
    }
}

impl Into<ErrorCode> for RpcErrorCodes {
    fn into(self) -> ErrorCode {
        (self as i64).into()
    }
}

#[rpc]
pub trait Rpc {
    #[rpc(name = "account_info", returns = "AccountInfoResp")]
    fn account_info(
        &self,
        addr: Address,
    ) -> Box<dyn futures01::Future<Item = AccountInfoResp, Error = Error> + Send>;

    #[rpc(name = "ethop_info")]
    fn ethop_info(&self, serial_id: u32) -> Result<ETHOpInfoResp>;

    #[rpc(name = "tx_info")]
    fn tx_info(&self, hash: TxHash) -> Result<TransactionInfoResp>;

    #[rpc(name = "tx_submit", returns = "TxHash")]
    fn tx_submit(
        &self,
        tx: Box<FranklinTx>,
        signature: Box<Option<TxEthSignature>>,
    ) -> Box<dyn futures01::Future<Item = TxHash, Error = Error> + Send>;

    #[rpc(name = "contract_address")]
    fn contract_address(&self) -> Result<ContractAddressResp>;

    /// "ETH" | #ERC20_ADDRESS => {Token}
    #[rpc(name = "tokens")]
    fn tokens(&self) -> Result<HashMap<String, Token>>;

    #[rpc(name = "get_tx_fee")]
    fn get_tx_fee(
        &self,
        tx_type: TxFeeTypes,
        amount: BigDecimal,
        token_like: TokenLike,
    ) -> Result<BigDecimal>;

    #[rpc(name = "get_confirmations_for_eth_op_amount", returns = "u64")]
    fn get_confirmations_for_eth_op_amount(&self) -> Result<u64>;

    #[rpc(name = "token_info")]
    fn token_info(&self, token_id: u16) -> Result<TokenInfoResp>;
}

#[derive(Clone)]
pub struct RpcApp {
    cache_of_executed_priority_operations: SharedLruCache<u32, StoredExecutedPriorityOperation>,
    cache_of_blocks_info: SharedLruCache<i64, BlockDetails>,
    cache_of_transaction_receipts: SharedLruCache<Vec<u8>, TxReceiptResponse>,

    pub mempool_request_sender: mpsc::Sender<MempoolRequest>,
    pub state_keeper_request_sender: mpsc::Sender<StateKeeperRequest>,
    pub eth_watcher_request_sender: mpsc::Sender<EthWatchRequest>,
    pub sign_verify_request_sender: mpsc::Sender<VerifyTxSignatureRequest>,

    pub connection_pool: ConnectionPool,

    pub confirmations_for_eth_event: u64,
}

impl RpcApp {
    pub fn new(
        config_options: &ConfigurationOptions,
        connection_pool: ConnectionPool,
        mempool_request_sender: mpsc::Sender<MempoolRequest>,
        state_keeper_request_sender: mpsc::Sender<StateKeeperRequest>,
        sign_verify_request_sender: mpsc::Sender<VerifyTxSignatureRequest>,
        eth_watcher_request_sender: mpsc::Sender<EthWatchRequest>,
    ) -> Self {
        let api_requests_caches_size = config_options.api_requests_caches_size;
        let confirmations_for_eth_event = config_options.confirmations_for_eth_event;

        RpcApp {
            cache_of_executed_priority_operations: SharedLruCache::new(api_requests_caches_size),
            cache_of_blocks_info: SharedLruCache::new(api_requests_caches_size),
            cache_of_transaction_receipts: SharedLruCache::new(api_requests_caches_size),

            connection_pool,

            mempool_request_sender,
            state_keeper_request_sender,
            sign_verify_request_sender,
            eth_watcher_request_sender,

            confirmations_for_eth_event,
        }
    }

    pub fn extend<T: Metadata, S: Middleware<T>>(self, io: &mut MetaIoHandler<T, S>) {
        io.extend_with(self.to_delegate())
    }

    /// Returns a message that user has to sign to send the transaction.
    /// If the transaction doesn't need a message signature, returns `None`.
    /// If any error is encountered during the message generation, returns `jsonrpc_core::Error`.
    fn get_tx_info_message_to_sign(&self, tx: &FranklinTx) -> Result<Option<String>> {
        match tx {
            FranklinTx::Transfer(tx) => Ok(Some(tx.get_ethereum_sign_message())),
            FranklinTx::Withdraw(tx) => Ok(Some(tx.get_ethereum_sign_message())),
            _ => Ok(None),
        }
    }
}

pub(crate) async fn get_ongoing_priority_ops(
    eth_watcher_request_sender: &mpsc::Sender<EthWatchRequest>,
) -> Result<Vec<(u64, PriorityOp)>> {
    let mut eth_watcher_request_sender = eth_watcher_request_sender.clone();

    let eth_watcher_response = oneshot::channel();

    // Get all the ongoing priority ops from the `EthWatcher`.
    eth_watcher_request_sender
        .send(EthWatchRequest::GetUnconfirmedQueueOps {
            resp: eth_watcher_response.0,
        })
        .await
        .map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: N/A",
                file!(),
                line!(),
                column!(),
                err
            );
            Error::internal_error()
        })?;

    eth_watcher_response
        .1
        .await
        .map_err(|_| Error::internal_error())
}

impl RpcApp {
    fn access_storage(&self) -> Result<StorageProcessor> {
        self.connection_pool
            .access_storage_fragile()
            .map_err(|_| Error::internal_error())
    }

    /// Async version of `get_ongoing_deposits` which does not use old futures as a return type.
    async fn get_ongoing_deposits_impl(&self, address: Address) -> Result<OngoingDepositsResp> {
        let confirmations_for_eth_event = self.confirmations_for_eth_event;

        let ongoing_ops = get_ongoing_priority_ops(&self.eth_watcher_request_sender).await?;

        let mut max_block_number = 0;

        // Filter only deposits for the requested address.
        // `map` is used after filter to find the max block number without an
        // additional list pass.
        let deposits: Vec<_> = ongoing_ops
            .into_iter()
            .filter(|(_block, op)| {
                if let FranklinPriorityOp::Deposit(deposit) = &op.data {
                    // Address may be set to either sender or recipient.
                    deposit.from == address || deposit.to == address
                } else {
                    false
                }
            })
            .map(|(block, op)| {
                if block > max_block_number {
                    max_block_number = block;
                }

                OngoingDeposit::new(block, op)
            })
            .collect();

        let estimated_deposits_approval_block = if !deposits.is_empty() {
            // We have to wait `confirmations_for_eth_event` blocks after the most
            // recent deposit operation.
            Some(max_block_number + confirmations_for_eth_event)
        } else {
            // No ongoing deposits => no estimated block.
            None
        };

        Ok(OngoingDepositsResp {
            address,
            deposits,
            confirmations_for_eth_event,
            estimated_deposits_approval_block,
        })
    }

    // cache access functions
    fn get_executed_priority_operation(
        &self,
        serial_id: u32,
    ) -> Result<Option<StoredExecutedPriorityOperation>> {
        let res =
            if let Some(executed_op) = self.cache_of_executed_priority_operations.get(&serial_id) {
                Some(executed_op)
            } else {
                let storage = self.access_storage()?;
                let executed_op = storage
                    .chain()
                    .operations_schema()
                    .get_executed_priority_operation(serial_id)
                    .map_err(|err| {
                        log::warn!(
                            "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                            file!(),
                            line!(),
                            column!(),
                            err,
                            serial_id,
                        );
                        Error::internal_error()
                    })?;

                if let Some(executed_op) = executed_op.clone() {
                    self.cache_of_executed_priority_operations
                        .insert(serial_id, executed_op);
                }

                executed_op
            };
        Ok(res)
    }

    fn get_block_info(&self, block_number: i64) -> Result<Option<BlockDetails>> {
        let res = if let Some(block) = self.cache_of_blocks_info.get(&block_number) {
            Some(block)
        } else {
            let storage = self.access_storage()?;
            let block = storage
                .chain()
                .block_schema()
                .find_block_by_height_or_hash(block_number.to_string());

            if let Some(block) = block.clone() {
                // Unverified blocks can still change, so we can't cache them.
                if block.verified_at.is_some() {
                    self.cache_of_blocks_info.insert(block_number, block);
                }
            }

            block
        };
        Ok(res)
    }

    fn get_tx_receipt(&self, tx_hash: TxHash) -> Result<Option<TxReceiptResponse>> {
        let res = if let Some(tx_receipt) = self
            .cache_of_transaction_receipts
            .get(&tx_hash.as_ref().to_vec())
        {
            Some(tx_receipt)
        } else {
            let storage = self.access_storage()?;
            let tx_receipt = storage
                .chain()
                .operations_ext_schema()
                .tx_receipt(tx_hash.as_ref())
                .map_err(|err| {
                    log::warn!(
                        "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                        file!(),
                        line!(),
                        column!(),
                        err,
                        tx_hash.to_string(),
                    );
                    Error::internal_error()
                })?;

            if let Some(tx_receipt) = tx_receipt.clone() {
                if tx_receipt.verified {
                    self.cache_of_transaction_receipts
                        .insert(tx_hash.as_ref().to_vec(), tx_receipt);
                }
            }

            tx_receipt
        };
        Ok(res)
    }
}

impl Rpc for RpcApp {
    fn account_info(
        &self,
        address: Address,
    ) -> Box<dyn futures01::Future<Item = AccountInfoResp, Error = Error> + Send> {
        let account = if let Ok(account) = (|| -> Result<_> {
            let storage = self.access_storage()?;
            let account = storage
                .chain()
                .account_schema()
                .account_state_by_address(&address)
                .map_err(|err| {
                    log::warn!(
                        "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                        file!(),
                        line!(),
                        column!(),
                        err,
                        address,
                    );
                    Error::internal_error()
                })?;
            Ok(account)
        })() {
            account
        } else {
            return Box::new(futures01::done(Err(Error::internal_error())));
        };

        let mut state_keeper_request_sender = self.state_keeper_request_sender.clone();
        let self_ = self.clone();
        let account_state_resp = async move {
            let state_keeper_response = oneshot::channel();
            state_keeper_request_sender
                .send(StateKeeperRequest::GetAccount(
                    address,
                    state_keeper_response.0,
                ))
                .await
                .map_err(|err| {
                    log::warn!(
                        "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                        file!(),
                        line!(),
                        column!(),
                        err,
                        address,
                    );
                    Error::internal_error()
                })?;
            let committed_account_state = state_keeper_response.1.await.map_err(|err| {
                log::warn!(
                    "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                    file!(),
                    line!(),
                    column!(),
                    err,
                    address,
                );
                Error::internal_error()
            })?;

            let (id, committed) = if let Some((id, account)) = committed_account_state {
                (Some(id), ResponseAccountState::try_to_restore(account)?)
            } else {
                (None, ResponseAccountState::default())
            };

            let verified = if let Some((_, account)) = account.verified {
                ResponseAccountState::try_to_restore(account)?
            } else {
                ResponseAccountState::default()
            };

            let depositing_ops = self_.get_ongoing_deposits_impl(address).await?;
            let depositing = DepositingAccountTokens::from_pending_ops(depositing_ops)?;

            Ok(AccountInfoResp {
                address,
                id,
                committed,
                verified,
                depositing,
            })
        };

        Box::new(account_state_resp.boxed().compat())
    }

    fn ethop_info(&self, serial_id: u32) -> Result<ETHOpInfoResp> {
        let executed_op = self.get_executed_priority_operation(serial_id)?;
        Ok(if let Some(executed_op) = executed_op {
            let block = self.get_block_info(executed_op.block_number)?;
            ETHOpInfoResp {
                executed: true,
                block: Some(BlockInfo {
                    block_number: executed_op.block_number,
                    committed: true,
                    verified: block.map(|b| b.verified_at.is_some()).unwrap_or_default(),
                }),
            }
        } else {
            ETHOpInfoResp {
                executed: false,
                block: None,
            }
        })
    }

    fn get_confirmations_for_eth_op_amount(&self) -> Result<u64> {
        Ok(self.confirmations_for_eth_event)
    }

    fn tx_info(&self, tx_hash: TxHash) -> Result<TransactionInfoResp> {
        let stored_receipt = self.get_tx_receipt(tx_hash)?;
        Ok(if let Some(stored_receipt) = stored_receipt {
            TransactionInfoResp {
                executed: true,
                success: Some(stored_receipt.success),
                fail_reason: stored_receipt.fail_reason,
                block: Some(BlockInfo {
                    block_number: stored_receipt.block_number,
                    committed: true,
                    verified: stored_receipt.verified,
                }),
            }
        } else {
            TransactionInfoResp {
                executed: false,
                success: None,
                fail_reason: None,
                block: None,
            }
        })
    }

    fn tx_submit(
        &self,
        tx: Box<FranklinTx>,
        signature: Box<Option<TxEthSignature>>,
    ) -> Box<dyn futures01::Future<Item = TxHash, Error = Error> + Send> {
        if tx.is_close() {
            return Box::new(futures01::future::err(Error {
                code: RpcErrorCodes::AccountCloseDisabled.into(),
                message: "Account close tx is disabled.".to_string(),
                data: None,
            }));
        }

        let msg_to_sign = match self.get_tx_info_message_to_sign(&tx) {
            Ok(res) => res,
            Err(e) => return Box::new(futures01::future::err(e)),
        };

        let mut mempool_sender = self.mempool_request_sender.clone();
        let sign_verify_channel = self.sign_verify_request_sender.clone();
        let mempool_resp = async move {
            let verified_tx = verify_tx_info_message_signature(
                &tx,
                *signature.clone(),
                msg_to_sign,
                sign_verify_channel,
            )
            .await?;

            let hash = tx.hash();
            let mempool_resp = oneshot::channel();
            mempool_sender
                .send(MempoolRequest::NewTx(Box::new(verified_tx), mempool_resp.0))
                .await
                .map_err(|err| {
                    log::warn!(
                        "[{}:{}:{}] Internal Server Error: '{}'; input: <Tx: '{:?}', signature: '{:?}'>",
                        file!(),
                        line!(),
                        column!(),
                        err,
                        tx,
                        signature,
                    );
                    Error::internal_error()
                })?;
            let tx_add_result = mempool_resp.1.await.unwrap_or(Err(TxAddError::Other));

            tx_add_result.map(|_| hash).map_err(|e| Error {
                code: RpcErrorCodes::from(e).into(),
                message: e.to_string(),
                data: None,
            })
        };

        Box::new(mempool_resp.boxed().compat())
    }

    fn contract_address(&self) -> Result<ContractAddressResp> {
        let storage = self.access_storage()?;
        let config = storage.config_schema().load_config().map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: N/A",
                file!(),
                line!(),
                column!(),
                err
            );
            Error::internal_error()
        })?;

        // `expect` calls below are safe, since not having the addresses in the server config
        // means a misconfiguration, server cannot operate in this condition.
        let main_contract = config
            .contract_addr
            .expect("Server config doesn't contain the main contract address");
        let gov_contract = config
            .gov_contract_addr
            .expect("Server config doesn't contain the gov contract address");
        Ok(ContractAddressResp {
            main_contract,
            gov_contract,
        })
    }

    fn tokens(&self) -> Result<HashMap<String, Token>> {
        Ok(HashMap::new()) // TODO ADE must be removed
    }

    fn get_tx_fee(
        &self,
        _tx_type: TxFeeTypes,
        amount: BigDecimal,
        _token_like: TokenLike,
    ) -> Result<BigDecimal> {
        // first approximation - just give 1 percent
        Ok(closest_packable_fee_amount(&floor_big_decimal(
            &(amount / BigDecimal::from(100)),
        )))
    }

    fn token_info(&self, token_id: u16) -> Result<TokenInfoResp> {
        log::debug!("Get token_info for token {}", token_id);
        let storage = self.access_storage()?;
        let history = storage.colexi_queries().get_transfer_history(token_id).map_err(|err| {
            log::error!(
                "[{}:{}:{}] Internal Server Error: '{}';",
                file!(),
                line!(),
                column!(),
                err
            );
            Error::internal_error()
        })?;
        let history: Vec<TransferHistory> = history.iter().map(|transfer| {
            TransferHistory {
                from: transfer.from.clone(),
                to: transfer.to.clone(),
                verified: transfer.proof_block_number.is_some(),
                block_number: transfer.block_number,
                date: chrono::DateTime::<chrono::Utc>::from_utc(transfer.created_at, chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                tx_hash: format!("0x{}", hex::encode(&transfer.tx_hash)),
            }
        }).collect();
        let owner = storage.colexi_queries().get_current_owner(token_id).map_err(|err| {
            log::error!(
                "[{}:{}:{}] Internal Server Error: '{}';",
                file!(),
                line!(),
                column!(),
                err
            );
            Error::internal_error()
        })?;
        let result = TokenInfoResp {
            history: history,
            owner: owner.map(|account| format!("0x{}", hex::encode(&account.address))),
        };

        return Ok(result);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start_rpc_server(
    config_options: ConfigurationOptions,
    connection_pool: ConnectionPool,
    mempool_request_sender: mpsc::Sender<MempoolRequest>,
    state_keeper_request_sender: mpsc::Sender<StateKeeperRequest>,
    sign_verify_request_sender: mpsc::Sender<VerifyTxSignatureRequest>,
    eth_watcher_request_sender: mpsc::Sender<EthWatchRequest>,
    panic_notify: mpsc::Sender<bool>,
) {
    let addr = config_options.json_rpc_http_server_address;
    std::thread::Builder::new()
        .name("json_rpc_http".to_string())
        .spawn(move || {
            let _panic_sentinel = ThreadPanicNotify(panic_notify);
            let mut io = IoHandler::new();

            let rpc_app = RpcApp::new(
                &config_options,
                connection_pool,
                mempool_request_sender,
                state_keeper_request_sender,
                sign_verify_request_sender,
                eth_watcher_request_sender,
            );
            rpc_app.extend(&mut io);

            let server = ServerBuilder::new(io).threads(8).start_http(&addr).unwrap();

            server.wait();
        })
        .expect("JSON-RPC http thread");
}

async fn verify_tx_info_message_signature(
    tx: &FranklinTx,
    signature: Option<TxEthSignature>,
    msg_to_sign: Option<String>,
    mut req_channel: mpsc::Sender<VerifyTxSignatureRequest>,
) -> Result<VerifiedTx> {
    fn rpc_message(error: TxAddError) -> Error {
        Error {
            code: RpcErrorCodes::from(error).into(),
            message: error.to_string(),
            data: None,
        }
    }

    let eth_sign_data = match msg_to_sign {
        Some(message_to_sign) => {
            let signature =
                signature.ok_or_else(|| rpc_message(TxAddError::MissingEthSignature))?;

            Some((signature, message_to_sign))
        }
        None => None,
    };

    let resp = oneshot::channel();

    let request = VerifyTxSignatureRequest {
        tx: tx.clone(),
        eth_sign_data,
        response: resp.0,
    };

    // Send the check request.
    req_channel.send(request).await.map_err(|err| {
        log::warn!(
            "[{}:{}:{}] Internal Server Error: '{}'; input: N/A",
            file!(),
            line!(),
            column!(),
            err
        );
        Error::internal_error()
    })?;

    // Wait for the check result.
    resp.1
        .await
        .map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: N/A",
                file!(),
                line!(),
                column!(),
                err
            );
            Error::internal_error()
        })?
        .map_err(rpc_message)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn tx_fee_type_serialization() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Query {
            tx_type: TxFeeTypes,
        }

        let cases = vec![
            (
                Query {
                    tx_type: TxFeeTypes::Withdraw,
                },
                r#"{"tx_type":"Withdraw"}"#,
            ),
            (
                Query {
                    tx_type: TxFeeTypes::Transfer,
                },
                r#"{"tx_type":"Transfer"}"#,
            ),
        ];
        for (query, json_str) in cases {
            let ser = serde_json::to_string(&query).expect("ser");
            assert_eq!(ser, json_str);
            let de = serde_json::from_str::<Query>(&ser).expect("de");
            assert_eq!(query, de);
        }
    }
}
