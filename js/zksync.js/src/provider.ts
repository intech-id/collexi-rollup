import {
    AbstractJSONRPCTransport,
    HTTPTransport,
    WSTransport
} from "./transport";
import { utils, ethers, Contract } from "ethers";
import {
    AccountState,
    Address,
    TokenLike,
    TransactionReceipt,
    PriorityOperationReceipt,
    ContractAddress,
    Tokens,
    TokenAddress,
    TxEthSignature
} from "./types";
import {
    isTokenETH,
    sleep,
    SYNC_GOV_CONTRACT_INTERFACE,
    SYNC_MAIN_CONTRACT_INTERFACE,
    TokenSet
} from "./utils";

export async function getDefaultProvider(
    network: "localhost" | "rinkeby" | "ropsten",
    transport: "WS" | "HTTP" = "WS"
): Promise<Provider> {
    if (network == "localhost") {
        if (transport == "WS") {
            return await Provider.newWebsocketProvider("ws://127.0.0.1:3031");
        } else if (transport == "HTTP") {
            return await Provider.newHttpProvider("http://127.0.0.1:3030");
        }
    } else if (network == "ropsten") {
        if (transport == "WS") {
            return await Provider.newWebsocketProvider(
                "wss://ropsten-api.zksync.dev/jsrpc-ws"
            );
        } else if (transport == "HTTP") {
            return await Provider.newHttpProvider(
                "https://ropsten-api.zksync.dev/jsrpc"
            );
        }
    } else if (network == "rinkeby") {
        if (transport == "WS") {
            return await Provider.newWebsocketProvider(
                "wss://rinkeby-api.zksync.dev/jsrpc-ws"
            );
        } else if (transport == "HTTP") {
            return await Provider.newHttpProvider(
                "https://rinkeby-api.zksync.dev/jsrpc"
            );
        }
    }
}

export class Provider {
    contractAddress: ContractAddress;
    public tokenSet: TokenSet;
    private constructor(public transport: AbstractJSONRPCTransport) {}

    static async newWebsocketProvider(address: string): Promise<Provider> {
        const transport = await WSTransport.connect(address);
        const provider = new Provider(transport);
        provider.contractAddress = await provider.getContractAddress();
        provider.tokenSet = new TokenSet(await provider.getTokens());
        return provider;
    }

    static async newHttpProvider(
        address: string = "http://127.0.0.1:3030"
    ): Promise<Provider> {
        const transport = new HTTPTransport(address);
        const provider = new Provider(transport);
        provider.contractAddress = await provider.getContractAddress();
        provider.tokenSet = new TokenSet(await provider.getTokens());
        return provider;
    }

    // return transaction hash (e.g. sync-tx:dead..beef)
    async submitTx(tx: any, signature?: TxEthSignature): Promise<string> {
        return await this.transport.request("tx_submit", [tx, signature]);
    }

    async getContractAddress(): Promise<ContractAddress> {
        return await this.transport.request("contract_address", null);
    }

    async getTokens(): Promise<Tokens> {
        return await this.transport.request("tokens", null);
    }

    async getState(address: Address): Promise<AccountState> {
        return await this.transport.request("account_info", [address]);
    }

    // get transaction status by its hash (e.g. 0xdead..beef)
    async getTxReceipt(txHash: string): Promise<TransactionReceipt> {
        return await this.transport.request("tx_info", [txHash]);
    }

    async getPriorityOpStatus(
        serialId: number
    ): Promise<PriorityOperationReceipt> {
        return await this.transport.request("ethop_info", [serialId]);
    }

    async getConfirmationsForEthOpAmount(): Promise<number> {
        return await this.transport.request(
            "get_confirmations_for_eth_op_amount",
            []
        );
    }

    async notifyPriorityOp(
        serialId: number,
        action: "COMMIT" | "VERIFY"
    ): Promise<PriorityOperationReceipt> {
        if (this.transport.subscriptionsSupported()) {
            return await new Promise(resolve => {
                const sub = this.transport.subscribe(
                    "ethop_subscribe",
                    [serialId, action],
                    "ethop_unsubscribe",
                    resp => {
                        sub.then(sub => sub.unsubscribe());
                        resolve(resp);
                    }
                );
            });
        } else {
            while (true) {
                const priorOpStatus = await this.getPriorityOpStatus(serialId);
                const notifyDone =
                    action == "COMMIT"
                        ? priorOpStatus.block && priorOpStatus.block.committed
                        : priorOpStatus.block && priorOpStatus.block.verified;
                if (notifyDone) {
                    return priorOpStatus;
                } else {
                    await sleep(3000);
                }
            }
        }
    }

    async notifyTransaction(
        hash: string,
        action: "COMMIT" | "VERIFY"
    ): Promise<TransactionReceipt> {
        if (this.transport.subscriptionsSupported()) {
            return await new Promise(resolve => {
                const sub = this.transport.subscribe(
                    "tx_subscribe",
                    [hash, action],
                    "tx_unsubscribe",
                    resp => {
                        sub.then(sub => sub.unsubscribe());
                        resolve(resp);
                    }
                );
            });
        } else {
            while (true) {
                const transactionStatus = await this.getTxReceipt(hash);
                const notifyDone =
                    action == "COMMIT"
                        ? transactionStatus.block &&
                          transactionStatus.block.committed
                        : transactionStatus.block &&
                          transactionStatus.block.verified;
                if (notifyDone) {
                    return transactionStatus;
                } else {
                    await sleep(3000);
                }
            }
        }
    }

    async getTransactionFee(
        txType: "Withdraw" | "Transfer",
        amount: utils.BigNumberish,
        tokenLike: TokenLike
    ): Promise<utils.BigNumber> {
        const transactionFee = await this.transport.request("get_tx_fee", [
            txType,
            amount.toString(),
            tokenLike
        ]);
        return utils.bigNumberify(transactionFee);
    }

    async disconnect() {
        return await this.transport.disconnect();
    }
}

export class ETHProxy {
    private governanceContract: Contract;
    private mainContract: Contract;

    constructor(
        private ethersProvider: ethers.providers.Provider,
        public contractAddress: ContractAddress
    ) {
        this.governanceContract = new Contract(
            this.contractAddress.govContract,
            SYNC_GOV_CONTRACT_INTERFACE,
            this.ethersProvider
        );

        this.mainContract = new Contract(
            this.contractAddress.mainContract,
            SYNC_MAIN_CONTRACT_INTERFACE,
            this.ethersProvider
        );
    }

    async resolveTokenId(token: TokenAddress): Promise<number> {
        if (isTokenETH(token)) {
            return 0;
        } else {
            const tokenId = await this.governanceContract.tokenIds(token);
            if (tokenId == 0) {
                throw new Error(`ERC20 token ${token} is not supported`);
            }
            return tokenId;
        }
    }
}
