use crate::mempool::MempoolRequest;
use crate::utils::shared_lru_cache::SharedLruCache;
use actix_cors::Cors;
use actix_web::{
    middleware,
    web::{self},
    App, HttpResponse, HttpServer, Result as ActixResult,
};
use futures::channel::mpsc;
use models::config_options::ThreadPanicNotify;
use models::node::{Account, AccountId, Address, ExecutedOperations, FranklinPriorityOp};
use models::NetworkStatus;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use storage::chain::block::records::BlockDetails;
use storage::chain::operations_ext::records::{PriorityOpReceiptResponse, TxReceiptResponse};
use storage::{ConnectionPool, StorageProcessor};
use tokio::{runtime::Runtime, time};
use web3::types::H160;

use super::rpc_server::get_ongoing_priority_ops;
use crate::eth_watch::EthWatchRequest;
use storage::chain::operations_ext::records::TransactionsHistoryItem;

#[derive(Default, Clone)]
struct SharedNetworkStatus(Arc<RwLock<NetworkStatus>>);

impl SharedNetworkStatus {
    fn read(&self) -> NetworkStatus {
        (*self.0.as_ref().read().unwrap()).clone()
    }
}

fn remove_prefix(query: &str) -> &str {
    if query.starts_with("0x") {
        &query[2..]
    } else if query.starts_with("sync-bl:") || query.starts_with("sync-tx:") {
        &query[8..]
    } else {
        &query
    }
}

fn try_parse_address(query: &str) -> Option<Address> {
    const ADDRESS_SIZE: usize = 20; // 20 bytes

    let query = remove_prefix(query);
    let b = hex::decode(query).ok()?;

    if b.len() == ADDRESS_SIZE {
        Some(Address::from_slice(&b))
    } else {
        None
    }
}

fn try_parse_hash(query: &str) -> Option<Vec<u8>> {
    const HASH_SIZE: usize = 32; // 32 bytes

    let query = remove_prefix(query);
    let b = hex::decode(query).ok()?;

    if b.len() == HASH_SIZE {
        Some(b)
    } else {
        None
    }
}

/// Caches used by REST API server.
#[derive(Debug, Clone)]
struct Caches {
    pub transaction_receipts: SharedLruCache<Vec<u8>, TxReceiptResponse>,
    pub priority_op_receipts: SharedLruCache<u32, PriorityOpReceiptResponse>,
    pub block_executed_ops: SharedLruCache<u32, Vec<ExecutedOperations>>,
    pub blocks_info: SharedLruCache<u32, BlockDetails>,
    pub blocks_by_height_or_hash: SharedLruCache<String, BlockDetails>,
}

impl Caches {
    pub fn new(caches_size: usize) -> Self {
        Self {
            transaction_receipts: SharedLruCache::new(caches_size),
            priority_op_receipts: SharedLruCache::new(caches_size),
            block_executed_ops: SharedLruCache::new(caches_size),
            blocks_info: SharedLruCache::new(caches_size),
            blocks_by_height_or_hash: SharedLruCache::new(caches_size),
        }
    }
}

/// AppState is a collection of records cloned by each thread to shara data between them
#[derive(Clone)]
struct AppState {
    caches: Caches,
    connection_pool: ConnectionPool,
    network_status: SharedNetworkStatus,
    contract_address: String,
    mempool_request_sender: mpsc::Sender<MempoolRequest>,
    eth_watcher_request_sender: mpsc::Sender<EthWatchRequest>,
}

impl AppState {
    fn access_storage(&self) -> ActixResult<StorageProcessor> {
        self.connection_pool
            .access_storage_fragile()
            .map_err(|err| {
                log::warn!(
                    "[{}:{}:{}] DB await timeout: '{}';",
                    file!(),
                    line!(),
                    column!(),
                    err,
                );
                HttpResponse::RequestTimeout().finish().into()
            })
    }

    // Spawns future updating SharedNetworkStatus in the current `actix::System`
    fn spawn_network_status_updater(&self, panic_notify: mpsc::Sender<bool>) {
        let state = self.clone();

        std::thread::Builder::new()
            .name("rest-state-updater".to_string())
            .spawn(move || {
                let _panic_sentinel = ThreadPanicNotify(panic_notify.clone());

                let mut runtime = Runtime::new().expect("tokio runtime creation");

                let state_update_task = async move {
                    let mut timer = time::interval(Duration::from_millis(1000));
                    loop {
                        timer.tick().await;

                        let storage = match state.connection_pool.access_storage() {
                            Ok(storage) => storage,
                            Err(err) => {
                                log::warn!("Unable to update the network status. Storage access failed: {}", err);
                                continue;
                            }
                        };

                        let last_verified = storage
                            .chain()
                            .block_schema()
                            .get_last_verified_block()
                            .unwrap_or(0);
                        let status = NetworkStatus {
                            next_block_at_max: None,
                            last_committed: storage
                                .chain()
                                .block_schema()
                                .get_last_committed_block()
                                .unwrap_or(0),
                            last_verified,
                            total_transactions: storage
                                .chain()
                                .stats_schema()
                                .count_total_transactions()
                                .unwrap_or(0),
                            outstanding_txs: storage
                                .chain()
                                .stats_schema()
                                .count_outstanding_proofs(last_verified)
                                .unwrap_or(0),
                        };

                        // save status to state
                        *state.network_status.0.as_ref().write().unwrap() = status;
                    }
                };
                runtime.block_on(state_update_task);
            })
            .expect("State update thread");
    }

    // cache access functions
    fn get_tx_receipt(
        &self,
        transaction_hash: Vec<u8>,
    ) -> Result<Option<TxReceiptResponse>, actix_web::error::Error> {
        if let Some(tx_receipt) = self.caches.transaction_receipts.get(&transaction_hash) {
            return Ok(Some(tx_receipt));
        }

        let storage = self.access_storage()?;
        let tx_receipt = storage
            .chain()
            .operations_ext_schema()
            .tx_receipt(transaction_hash.as_slice())
            .unwrap_or(None);

        if let Some(tx_receipt) = tx_receipt.clone() {
            // Unverified blocks can still change, so we can't cache them.
            if tx_receipt.verified {
                self.caches
                    .transaction_receipts
                    .insert(transaction_hash, tx_receipt);
            }
        }

        Ok(tx_receipt)
    }

    fn get_priority_op_receipt(
        &self,
        id: u32,
    ) -> Result<PriorityOpReceiptResponse, actix_web::error::Error> {
        if let Some(receipt) = self.caches.priority_op_receipts.get(&id) {
            return Ok(receipt);
        }

        let storage = self.access_storage()?;
        let receipt = storage
            .chain()
            .operations_ext_schema()
            .get_priority_op_receipt(id)
            .map_err(|err| {
                log::warn!(
                    "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                    file!(),
                    line!(),
                    column!(),
                    err,
                    id,
                );
                HttpResponse::InternalServerError().finish()
            })?;

        // Unverified blocks can still change, so we can't cache them.
        if receipt.verified {
            self.caches.priority_op_receipts.insert(id, receipt.clone());
        }

        Ok(receipt)
    }

    fn get_block_executed_ops(
        &self,
        block_id: u32,
    ) -> Result<Vec<ExecutedOperations>, actix_web::error::Error> {
        if let Some(executed_ops) = self.caches.block_executed_ops.get(&block_id) {
            return Ok(executed_ops);
        }

        let storage = self.access_storage()?;
        let executed_ops = storage
            .chain()
            .block_schema()
            .get_block_executed_ops(block_id)
            .map_err(|err| {
                log::warn!(
                    "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                    file!(),
                    line!(),
                    column!(),
                    err,
                    block_id,
                );
                HttpResponse::InternalServerError().finish()
            })?;

        if let Ok(block_details) = storage.chain().block_schema().load_block_range(block_id, 1) {
            // Unverified blocks can still change, so we can't cache them.
            if !block_details.is_empty() && block_details[0].verified_at.is_some() {
                self.caches
                    .block_executed_ops
                    .insert(block_id, executed_ops.clone());
            }
        }

        Ok(executed_ops)
    }

    fn get_block_info(
        &self,
        block_id: u32,
    ) -> Result<Option<BlockDetails>, actix_web::error::Error> {
        if let Some(block) = self.caches.blocks_info.get(&block_id) {
            return Ok(Some(block));
        }

        let storage = self.access_storage()?;
        let mut blocks = storage
            .chain()
            .block_schema()
            .load_block_range(block_id, 1)
            .map_err(|err| {
                log::warn!(
                    "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                    file!(),
                    line!(),
                    column!(),
                    err,
                    block_id,
                );
                HttpResponse::InternalServerError().finish()
            })?;

        if !blocks.is_empty() && blocks[0].verified_at.is_some() {
            self.caches.blocks_info.insert(block_id, blocks[0].clone());
        }

        Ok(blocks.pop())
    }

    fn get_block_by_height_or_hash(
        &self,
        query: String,
    ) -> Result<Option<BlockDetails>, actix_web::error::Error> {
        if let Some(block) = self.caches.blocks_by_height_or_hash.get(&query) {
            return Ok(Some(block));
        }

        let storage = self.access_storage()?;
        let block = storage
            .chain()
            .block_schema()
            .find_block_by_height_or_hash(query.clone());

        if let Some(block) = block.clone() {
            if block.verified_at.is_some() {
                self.caches.blocks_by_height_or_hash.insert(query, block);
            }
        }

        Ok(block)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestnetConfigResponse {
    contract_address: String,
}

fn handle_get_testnet_config(data: web::Data<AppState>) -> ActixResult<HttpResponse> {
    let contract_address = data.contract_address.clone();
    Ok(HttpResponse::Ok().json(TestnetConfigResponse { contract_address }))
}

fn handle_get_network_status(data: web::Data<AppState>) -> ActixResult<HttpResponse> {
    let network_status = data.network_status.read();
    Ok(HttpResponse::Ok().json(network_status))
}

#[derive(Debug, Serialize)]
struct AccountStateResponse {
    // None if account is not created yet.
    id: Option<AccountId>,
    commited: Account,
    verified: Account,
}

fn handle_get_account_state(
    data: web::Data<AppState>,
    account_address: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let account_address =
        try_parse_address(&account_address).ok_or_else(|| HttpResponse::BadRequest().finish())?;

    let storage = data.access_storage()?;

    let (id, verified, commited) = {
        let stored_account_state = storage
            .chain()
            .account_schema()
            .account_state_by_address(&account_address)
            .map_err(|err| {
                log::warn!(
                    "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                    file!(),
                    line!(),
                    column!(),
                    err,
                    account_address,
                );
                HttpResponse::InternalServerError().finish()
            })?;

        let empty_state = |address: &Address| {
            let mut acc = Account::default();
            acc.address = *address;
            acc
        };

        let id = stored_account_state.committed.as_ref().map(|(id, _)| *id);
        let committed = stored_account_state
            .committed
            .map(|(_, acc)| acc)
            .unwrap_or_else(|| empty_state(&account_address));
        let verified = stored_account_state
            .verified
            .map(|(_, acc)| acc)
            .unwrap_or_else(|| empty_state(&account_address));

        (id, verified, committed)
    };

    let res = AccountStateResponse {
        id,
        commited,
        verified,
    };

    Ok(HttpResponse::Ok().json(res))
}

// TODO ADE must be removed
fn handle_get_tokens(data: web::Data<AppState>) -> ActixResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(Vec::<u16>::new()))
}

fn handle_get_account_transactions_history(
    data: web::Data<AppState>,
    request_path: web::Path<(Address, u64, u64)>,
) -> ActixResult<HttpResponse> {
    let (address, mut offset, mut limit) = request_path.into_inner();

    const MAX_LIMIT: u64 = 100;
    if limit > MAX_LIMIT {
        return Err(HttpResponse::BadRequest().finish().into());
    }

    let eth_watcher_request_sender = data.eth_watcher_request_sender.clone();

    // Fetch ongoing deposits, since they must be reported within the transactions history.
    let mut ongoing_ops = futures::executor::block_on(async move {
        get_ongoing_priority_ops(&eth_watcher_request_sender).await
    })
    .map_err(|err| {
        log::warn!(
            "[{}:{}:{}] Internal Server Error: '{}'; input: ({}, {}, {})",
            file!(),
            line!(),
            column!(),
            err,
            address,
            offset,
            limit,
        );
        HttpResponse::InternalServerError().finish()
    })?;

    // Sort operations by block number in a reverse order (so the newer ones are on top).
    // Note that we call `cmp` on `rhs` to achieve that.
    ongoing_ops.sort_by(|lhs, rhs| rhs.0.cmp(&lhs.0));

    // Filter only deposits for the requested address.
    // `map` is used after filter to find the max block number without an
    // additional list pass.
    // `take` is used last to limit the amount of entries.
    let mut transactions_history: Vec<_> = ongoing_ops
        .iter()
        .filter(|(_block, op)| match &op.data {
            FranklinPriorityOp::Deposit(deposit) => {
                // Address may be set to either sender or recipient.
                deposit.from == address || deposit.to == address
            }
            _ => false,
        })
        .map(|(_block, op)| {
            let deposit = op.data.try_get_deposit().unwrap();
            let hash_str = format!("0x{}", hex::encode(&op.eth_hash));
            let pq_id = Some(op.serial_id as i64);

            // Account ID may not exist for depositing ops, so it'll be `null`.
            let account_id: Option<u32> = None;

            // Copy the JSON representation of the executed tx so the appearance
            // will be the same as for txs from storage.
            let tx_json = serde_json::json!({
                "account_id": account_id,
                "priority_op": {
                    "from": deposit.from,
                    "to": deposit.to,
                    "token_id": deposit.token_id
                },
                "type": "Deposit"
            });

            // As the time of creation is indefinite, we always will provide the current time.
            let current_time = chrono::Utc::now();
            let naitve_current_time =
                chrono::NaiveDateTime::from_timestamp(current_time.timestamp(), 0);

            TransactionsHistoryItem {
                hash: Some(hash_str),
                pq_id,
                tx: tx_json,
                success: None,
                fail_reason: None,
                commited: false,
                verified: false,
                created_at: naitve_current_time,
            }
        })
        .skip(offset as usize)
        .take(limit as usize)
        .collect();

    if !transactions_history.is_empty() {
        // We've taken at least one transaction, this means
        // offset is consumed completely, and limit is reduced.
        offset = 0;
        limit -= transactions_history.len() as u64;
    } else {
        // reduce offset by the number of pending deposits
        // that are soon to be added to the db.
        let num_account_ongoing_deposits = ongoing_ops
            .iter()
            .filter(|(_block, op)| match &op.data {
                FranklinPriorityOp::Deposit(deposit) => {
                    // Address may be set to either sender or recipient.
                    deposit.from == address || deposit.to == address
                }
                _ => false,
            })
            .count() as u64;

        offset = offset.saturating_sub(num_account_ongoing_deposits);
    }

    let storage = data.access_storage()?;
    let mut storage_transactions = storage
        .chain()
        .operations_ext_schema()
        .get_account_transactions_history(&address, offset, limit)
        .map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: ({}, {}, {})",
                file!(),
                line!(),
                column!(),
                err,
                address,
                offset,
                limit,
            );
            HttpResponse::InternalServerError().finish()
        })?;

    transactions_history.append(&mut storage_transactions);

    Ok(HttpResponse::Ok().json(transactions_history))
}

fn handle_get_executed_transaction_by_hash(
    data: web::Data<AppState>,
    tx_hash_hex: web::Path<String>,
) -> ActixResult<HttpResponse> {
    if tx_hash_hex.len() < 2 {
        return Err(HttpResponse::BadRequest().finish().into());
    }
    let transaction_hash = hex::decode(&tx_hash_hex.into_inner()[2..])
        .map_err(|_| HttpResponse::BadRequest().finish())?;

    let tx_receipt = data.get_tx_receipt(transaction_hash)?;

    if let Some(tx) = tx_receipt {
        Ok(HttpResponse::Ok().json(tx))
    } else {
        Ok(HttpResponse::Ok().json(()))
    }
}

fn handle_get_tx_by_hash(
    data: web::Data<AppState>,
    hash_hex_with_prefix: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let hash =
        try_parse_hash(&hash_hex_with_prefix).ok_or_else(|| HttpResponse::BadRequest().finish())?;
    let storage = data.access_storage()?;

    let res = storage
        .chain()
        .operations_ext_schema()
        .get_tx_by_hash(hash.as_slice())
        .map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                file!(),
                line!(),
                column!(),
                err,
                hex::encode(&hash),
            );
            HttpResponse::InternalServerError().finish()
        })?;

    Ok(HttpResponse::Ok().json(res))
}

fn handle_get_priority_op_receipt(
    data: web::Data<AppState>,
    id: web::Path<u32>,
) -> ActixResult<HttpResponse> {
    let id = id.into_inner();
    let receipt = data.get_priority_op_receipt(id)?;

    Ok(HttpResponse::Ok().json(receipt))
}

fn handle_get_transaction_by_id(
    data: web::Data<AppState>,
    path: web::Path<(u32, u32)>,
) -> ActixResult<HttpResponse> {
    let (block_id, tx_id) = path.into_inner();

    let exec_ops = data.get_block_executed_ops(block_id)?;

    if let Some(exec_op) = exec_ops.get(tx_id as usize) {
        Ok(HttpResponse::Ok().json(exec_op))
    } else {
        Err(HttpResponse::NotFound().finish().into())
    }
}

#[derive(Deserialize)]
struct HandleBlocksQuery {
    max_block: Option<u32>,
    limit: Option<u32>,
}

fn handle_get_blocks(
    data: web::Data<AppState>,
    query: web::Query<HandleBlocksQuery>,
) -> ActixResult<HttpResponse> {
    let max_block = query.max_block.unwrap_or(999_999_999);
    let limit = query.limit.unwrap_or(20);
    if limit > 100 {
        return Err(HttpResponse::BadRequest().finish().into());
    }
    let storage = data.access_storage()?;

    let resp = storage
        .chain()
        .block_schema()
        .load_block_range(max_block, limit)
        .map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: ({}, {})",
                file!(),
                line!(),
                column!(),
                err,
                max_block,
                limit,
            );
            HttpResponse::InternalServerError().finish()
        })?;
    Ok(HttpResponse::Ok().json(resp))
}

fn handle_get_block_by_id(
    data: web::Data<AppState>,
    block_id: web::Path<u32>,
) -> ActixResult<HttpResponse> {
    let block_id = block_id.into_inner();
    let block = data.get_block_info(block_id)?;
    if let Some(block) = block {
        Ok(HttpResponse::Ok().json(block))
    } else {
        Err(HttpResponse::NotFound().finish().into())
    }
}

fn handle_get_block_transactions(
    data: web::Data<AppState>,
    path: web::Path<u32>,
) -> ActixResult<HttpResponse> {
    let block_number = path.into_inner();

    let storage = data.access_storage()?;

    let txs = storage
        .chain()
        .block_schema()
        .get_block_transactions(block_number)
        .map_err(|err| {
            log::warn!(
                "[{}:{}:{}] Internal Server Error: '{}'; input: {}",
                file!(),
                line!(),
                column!(),
                err,
                block_number,
            );
            HttpResponse::InternalServerError().finish()
        })?;

    Ok(HttpResponse::Ok().json(txs))
}

#[derive(Deserialize)]
struct BlockExplorerSearchQuery {
    query: String,
}

fn handle_block_explorer_search(
    data: web::Data<AppState>,
    query: web::Query<BlockExplorerSearchQuery>,
) -> ActixResult<HttpResponse> {
    let query = query.into_inner().query;
    let block = data.get_block_by_height_or_hash(query)?;

    if let Some(block) = block {
        Ok(HttpResponse::Ok().json(block))
    } else {
        Err(HttpResponse::NotFound().finish().into())
    }
}

fn start_server(state: AppState, bind_to: SocketAddr) {
    HttpServer::new(move || {
        App::new()
            .data(state.clone())
            .wrap(middleware::Logger::default())
            .wrap(Cors::new().send_wildcard().max_age(3600))
            .service(
                web::scope("/api/v0.1")
                    .route(
                        "/blocks/{block_id}/transactions",
                        web::get().to(handle_get_block_transactions),
                    )
                    .route("/testnet_config", web::get().to(handle_get_testnet_config))
                    .route("/status", web::get().to(handle_get_network_status))
                    .route(
                        "/account/{address}",
                        web::get().to(handle_get_account_state),
                    )
                    .route("/tokens", web::get().to(handle_get_tokens))
                    .route(
                        "/account/{address}/history/{offset}/{limit}",
                        web::get().to(handle_get_account_transactions_history),
                    )
                    .route(
                        "/transactions/{tx_hash}",
                        web::get().to(handle_get_executed_transaction_by_hash),
                    )
                    .route(
                        "/transactions_all/{tx_hash}",
                        web::get().to(handle_get_tx_by_hash),
                    )
                    .route(
                        "/priority_operations/{pq_id}/",
                        web::get().to(handle_get_priority_op_receipt),
                    )
                    .route(
                        "/blocks/{block_id}/transactions/{tx_id}",
                        web::get().to(handle_get_transaction_by_id),
                    )
                    .route(
                        "/blocks/{block_id}/transactions",
                        web::get().to(handle_get_block_transactions),
                    )
                    .route("/blocks/{block_id}", web::get().to(handle_get_block_by_id))
                    .route("/blocks", web::get().to(handle_get_blocks))
                    .route("/search", web::get().to(handle_block_explorer_search)),
            )
            // Endpoint needed for js isReachable
            .route(
                "/favicon.ico",
                web::get().to(|| HttpResponse::Ok().finish()),
            )
    })
    .bind(bind_to)
    .unwrap()
    .shutdown_timeout(1)
    .start();
}

/// Start HTTP REST API
pub(super) fn start_server_thread_detached(
    connection_pool: ConnectionPool,
    listen_addr: SocketAddr,
    contract_address: H160,
    mempool_request_sender: mpsc::Sender<MempoolRequest>,
    eth_watcher_request_sender: mpsc::Sender<EthWatchRequest>,
    panic_notify: mpsc::Sender<bool>,
    api_requests_caches_size: usize,
) {
    std::thread::Builder::new()
        .name("actix-rest-api".to_string())
        .spawn(move || {
            let _panic_sentinel = ThreadPanicNotify(panic_notify.clone());

            let runtime = actix_rt::System::new("api-server");

            let state = AppState {
                caches: Caches::new(api_requests_caches_size),
                connection_pool,
                network_status: SharedNetworkStatus::default(),
                contract_address: format!("{:?}", contract_address),
                mempool_request_sender,
                eth_watcher_request_sender,
            };
            state.spawn_network_status_updater(panic_notify);

            start_server(state, listen_addr);
            runtime.run().unwrap_or_default();
        })
        .expect("Api server thread");
}
