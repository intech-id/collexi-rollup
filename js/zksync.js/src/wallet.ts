import { Contract, ContractTransaction, ethers, utils } from "ethers";
import { ETHProxy, Provider } from "./provider";
import { Signer } from "./signer";
import {
    AccountState,
    Address,
    TokenLike,
    Nonce,
    PriorityOperationReceipt,
    TransactionReceipt,
    PubKeyHash,
    TxEthSignature,
    ChangePubKey
} from "./types";
import {
    ERC20_APPROVE_TRESHOLD,
    IERC20_INTERFACE,
    isTokenETH,
    MAX_ERC20_APPROVE_AMOUNT,
    signChangePubkeyMessage,
    getEthSignatureType,
    SYNC_MAIN_CONTRACT_INTERFACE
} from "./utils";

// Our MetaMask users sometimes use custom gas price values,
// which we can't know. We use this constant to assure that
// gasprice from our calculations isn't smaller than actually used one.
const metamaskIncreaseGasPriceFactor = 10;

class ZKSyncTxError extends Error {
    constructor(
        message: string,
        public value: PriorityOperationReceipt | TransactionReceipt
    ) {
        super(message);
    }
}

export class Wallet {
    public provider: Provider;

    private constructor(
        public ethSigner: ethers.Signer,
        public cachedAddress: Address,
        public signer?: Signer,
        public accountId?: number,
        public ethSignatureType?: "EthereumSignature" | "EIP1271Signature"
    ) {}

    connect(provider: Provider) {
        this.provider = provider;
        return this;
    }

    static async fromEthSigner(
        ethWallet: ethers.Signer,
        provider: Provider,
        signer?: Signer,
        accountId?: number,
        ethSignatureType?: "EthereumSignature" | "EIP1271Signature"
    ): Promise<Wallet> {
        if (signer == undefined) {
            const signerResult = await Signer.fromETHSignature(ethWallet);
            signer = signerResult.signer;
            ethSignatureType =
                ethSignatureType || signerResult.ethSignatureType;
        } else if (ethSignatureType == undefined) {
            throw new Error(
                "If you passed signer, you must also pass ethSignatureType."
            );
        }

        const wallet = new Wallet(
            ethWallet,
            await ethWallet.getAddress(),
            signer,
            accountId,
            ethSignatureType
        );

        wallet.connect(provider);
        return wallet;
    }

    static async fromEthSignerNoKeys(
        ethWallet: ethers.Signer,
        provider: Provider,
        accountId?: number,
        ethSignatureType?: "EthereumSignature" | "EIP1271Signature"
    ): Promise<Wallet> {
        const wallet = new Wallet(
            ethWallet,
            await ethWallet.getAddress(),
            undefined,
            accountId,
            ethSignatureType
        );
        wallet.connect(provider);
        return wallet;
    }

    async getEthMessageSignature(message: string): Promise<TxEthSignature> {
        const signature = await this.ethSigner.signMessage(message);

        if (this.ethSignatureType == undefined) {
            const address = await this.ethSigner.getAddress();
            this.ethSignatureType = getEthSignatureType(
                message,
                signature,
                address
            );
        }

        return { type: this.ethSignatureType, signature };
    }

    async syncTransfer(transfer: {
        to: Address;
        token: TokenLike;
        amount: utils.BigNumberish;
        fee: utils.BigNumberish;
        nonce?: Nonce;
    }): Promise<Transaction> {
        if (!this.signer) {
            throw new Error(
                "ZKSync signer is required for sending zksync transactions."
            );
        }

        await this.setRequiredAccountIdFromServer("Transfer funds");

        const tokenId = await this.provider.tokenSet.resolveTokenId(
            transfer.token
        );
        const nonce =
            transfer.nonce != null
                ? await this.getNonce(transfer.nonce)
                : await this.getNonce();
        const transactionData = {
            accountId: this.accountId,
            from: this.address(),
            to: transfer.to,
            tokenId,
            amount: transfer.amount,
            fee: transfer.fee,
            nonce
        };

        const stringAmount = utils.formatEther(transfer.amount);
        const stringFee = utils.formatEther(transfer.fee);
        const stringToken = await this.provider.tokenSet.resolveTokenSymbol(
            transfer.token
        );
        const humanReadableTxInfo =
            `Transfer ${stringAmount} ${stringToken}\n` +
            `To: ${transfer.to.toLowerCase()}\n` +
            `Nonce: ${nonce}\n` +
            `Fee: ${stringFee} ${stringToken}\n` +
            `Account Id: ${this.accountId}`;

        const txMessageEthSignature = await this.getEthMessageSignature(
            humanReadableTxInfo
        );

        const signedTransferTransaction = this.signer.signSyncTransfer(
            transactionData
        );

        const transactionHash = await this.provider.submitTx(
            signedTransferTransaction,
            txMessageEthSignature
        );
        return new Transaction(
            signedTransferTransaction,
            transactionHash,
            this.provider
        );
    }

    async withdrawFromSyncToEthereum(withdraw: {
        ethAddress: string;
        token: TokenLike;
        amount: utils.BigNumberish;
        fee: utils.BigNumberish;
        nonce?: Nonce;
    }): Promise<Transaction> {
        if (!this.signer) {
            throw new Error(
                "ZKSync signer is required for sending zksync transactions."
            );
        }
        await this.setRequiredAccountIdFromServer("Withdraw funds");

        const tokenId = await this.provider.tokenSet.resolveTokenId(
            withdraw.token
        );
        const nonce =
            withdraw.nonce != null
                ? await this.getNonce(withdraw.nonce)
                : await this.getNonce();
        const transactionData = {
            accountId: this.accountId,
            from: this.address(),
            ethAddress: withdraw.ethAddress,
            tokenId,
            amount: withdraw.amount,
            fee: withdraw.fee,
            nonce
        };

        const stringAmount = utils.formatEther(withdraw.amount);
        const stringFee = utils.formatEther(withdraw.fee);
        const stringToken = await this.provider.tokenSet.resolveTokenSymbol(
            withdraw.token
        );
        const humanReadableTxInfo =
            `Withdraw ${stringAmount} ${stringToken}\n` +
            `To: ${withdraw.ethAddress.toLowerCase()}\n` +
            `Nonce: ${nonce}\n` +
            `Fee: ${stringFee} ${stringToken}\n` +
            `Account Id: ${this.accountId}`;

        const txMessageEthSignature = await this.getEthMessageSignature(
            humanReadableTxInfo
        );

        const signedWithdrawTransaction = this.signer.signSyncWithdraw(
            transactionData
        );

        const submitResponse = await this.provider.submitTx(
            signedWithdrawTransaction,
            txMessageEthSignature
        );
        return new Transaction(
            signedWithdrawTransaction,
            submitResponse,
            this.provider
        );
    }

    async isSigningKeySet(): Promise<boolean> {
        if (!this.signer) {
            throw new Error(
                "ZKSync signer is required for current pubkey calculation."
            );
        }
        const currentPubKeyHash = await this.getCurrentPubKeyHash();
        const signerPubKeyHash = this.signer.pubKeyHash();
        return currentPubKeyHash === signerPubKeyHash;
    }

    async setSigningKey(
        nonce: Nonce = "committed",
        onchainAuth = false
    ): Promise<Transaction> {
        if (!this.signer) {
            throw new Error(
                "ZKSync signer is required for current pubkey calculation."
            );
        }

        const currentPubKeyHash = await this.getCurrentPubKeyHash();
        const newPubKeyHash = this.signer.pubKeyHash();

        if (currentPubKeyHash === newPubKeyHash) {
            throw new Error("Current signing key is already set");
        }

        await this.setRequiredAccountIdFromServer("Set Signing Key");

        const numNonce = await this.getNonce(nonce);
        const ethSignature = onchainAuth
            ? null
            : await signChangePubkeyMessage(
                  this.ethSigner,
                  newPubKeyHash,
                  numNonce,
                  this.accountId
              );

        const txData: ChangePubKey = {
            type: "ChangePubKey",
            accountId: this.accountId,
            account: this.address(),
            newPkHash: this.signer.pubKeyHash(),
            nonce: numNonce,
            ethSignature
        };

        const transactionHash = await this.provider.submitTx(txData);
        return new Transaction(txData, transactionHash, this.provider);
    }

    async onchainAuthSigningKey(
        nonce: Nonce = "committed",
        ethTxOptions?: ethers.providers.TransactionRequest
    ): Promise<ContractTransaction> {
        if (!this.signer) {
            throw new Error(
                "ZKSync signer is required for current pubkey calculation."
            );
        }

        const currentPubKeyHash = await this.getCurrentPubKeyHash();
        const newPubKeyHash = this.signer.pubKeyHash();

        if (currentPubKeyHash == newPubKeyHash) {
            throw new Error("Current PubKeyHash is the same as new");
        }

        const numNonce = await this.getNonce(nonce);

        const mainZkSyncContract = new Contract(
            this.provider.contractAddress.mainContract,
            SYNC_MAIN_CONTRACT_INTERFACE,
            this.ethSigner
        );

        const ethTransaction = await mainZkSyncContract.setAuthPubkeyHash(
            newPubKeyHash.replace("sync:", "0x"),
            numNonce,
            {
                gasLimit: utils.bigNumberify("200000"),
                ...ethTxOptions
            }
        );

        return ethTransaction;
    }

    async getCurrentPubKeyHash(): Promise<PubKeyHash> {
        return (await this.provider.getState(this.address())).committed
            .pubKeyHash;
    }

    async getNonce(nonce: Nonce = "committed"): Promise<number> {
        if (nonce == "committed") {
            return (await this.provider.getState(this.address())).committed
                .nonce;
        } else if (typeof nonce == "number") {
            return nonce;
        }
    }

    async getAccountId(): Promise<number | undefined> {
        return (await this.provider.getState(this.address())).id;
    }

    address(): Address {
        return this.cachedAddress;
    }

    async getAccountState(): Promise<AccountState> {
        return this.provider.getState(this.address());
    }

    async getBalance(
        token: TokenLike,
        type: "committed" | "verified" = "committed"
    ): Promise<utils.BigNumber> {
        const accountState = await this.getAccountState();
        const tokenSymbol = this.provider.tokenSet.resolveTokenSymbol(token);
        let balance;
        if (type === "committed") {
            balance = accountState.committed.balances[tokenSymbol] || "0";
        } else {
            balance = accountState.verified.balances[tokenSymbol] || "0";
        }
        return utils.bigNumberify(balance);
    }

    async getEthereumBalance(token: TokenLike): Promise<utils.BigNumber> {
        let balance: utils.BigNumber;
        if (isTokenETH(token)) {
            balance = await this.ethSigner.provider.getBalance(
                this.cachedAddress
            );
        } else {
            const erc20contract = new Contract(
                this.provider.tokenSet.resolveTokenAddress(token),
                IERC20_INTERFACE,
                this.ethSigner
            );
            balance = await erc20contract.balanceOf(this.cachedAddress);
        }
        return balance;
    }

    async isERC20DepositsApproved(token: TokenLike): Promise<boolean> {
        if (isTokenETH(token)) {
            throw Error("ETH token does not need approval.");
        }
        const tokenAddress = this.provider.tokenSet.resolveTokenAddress(token);
        const erc20contract = new Contract(
            tokenAddress,
            IERC20_INTERFACE,
            this.ethSigner
        );
        const currentAllowance = await erc20contract.allowance(
            this.address(),
            this.provider.contractAddress.mainContract
        );
        return utils.bigNumberify(currentAllowance).gte(ERC20_APPROVE_TRESHOLD);
    }

    async approveERC20TokenDeposits(
        token: TokenLike
    ): Promise<ContractTransaction> {
        if (isTokenETH(token)) {
            throw Error("ETH token does not need approval.");
        }
        const tokenAddress = this.provider.tokenSet.resolveTokenAddress(token);
        const erc20contract = new Contract(
            tokenAddress,
            IERC20_INTERFACE,
            this.ethSigner
        );

        return erc20contract.approve(
            this.provider.contractAddress.mainContract,
            MAX_ERC20_APPROVE_AMOUNT
        );
    }

    async depositToSyncFromEthereum(deposit: {
        depositTo: Address;
        token: TokenLike;
        amount: utils.BigNumberish;
        ethTxOptions?: ethers.providers.TransactionRequest;
        approveDepositAmountForERC20?: boolean;
    }): Promise<ETHOperation> {
        const gasPrice = await this.ethSigner.provider.getGasPrice();

        const ethProxy = new ETHProxy(
            this.ethSigner.provider,
            this.provider.contractAddress
        );

        const mainZkSyncContract = new Contract(
            this.provider.contractAddress.mainContract,
            SYNC_MAIN_CONTRACT_INTERFACE,
            this.ethSigner
        );

        let ethTransaction;

        if (isTokenETH(deposit.token)) {
            ethTransaction = await mainZkSyncContract.depositETH(
                deposit.depositTo,
                {
                    value: utils.bigNumberify(deposit.amount),
                    gasLimit: utils.bigNumberify("200000"),
                    gasPrice,
                    ...deposit.ethTxOptions
                }
            );
        } else {
            const tokenAddress = this.provider.tokenSet.resolveTokenAddress(
                deposit.token
            );
            // ERC20 token deposit
            const erc20contract = new Contract(
                tokenAddress,
                IERC20_INTERFACE,
                this.ethSigner
            );
            if (deposit.approveDepositAmountForERC20) {
                const approveTx = await erc20contract.approve(
                    this.provider.contractAddress.mainContract,
                    deposit.amount
                );
                ethTransaction = await mainZkSyncContract.depositERC20(
                    tokenAddress,
                    deposit.amount,
                    deposit.depositTo,
                    {
                        gasLimit: utils.bigNumberify("250000"),
                        nonce: approveTx.nonce + 1,
                        gasPrice,
                        ...deposit.ethTxOptions
                    }
                );
            } else {
                if (!(await this.isERC20DepositsApproved(deposit.token))) {
                    throw Error("ERC20 deposit should be approved.");
                }
                ethTransaction = await mainZkSyncContract.depositERC20(
                    tokenAddress,
                    deposit.amount,
                    deposit.depositTo,
                    {
                        gasLimit: utils.bigNumberify("250000"),
                        gasPrice,
                        ...deposit.ethTxOptions
                    }
                );
            }
        }

        return new ETHOperation(ethTransaction, this.provider);
    }

    async emergencyWithdraw(withdraw: {
        token: TokenLike;
        accountId?: number;
        ethTxOptions?: ethers.providers.TransactionRequest;
    }): Promise<ETHOperation> {
        const gasPrice = await this.ethSigner.provider.getGasPrice();
        const ethProxy = new ETHProxy(
            this.ethSigner.provider,
            this.provider.contractAddress
        );

        let accountId;
        if (withdraw.accountId != null) {
            accountId = withdraw.accountId;
        } else if (this.accountId !== undefined) {
            accountId = this.accountId;
        } else {
            const accountState = await this.getAccountState();
            if (!accountState.id) {
                throw new Error(
                    "Can't resolve account id from the zkSync node"
                );
            }
            accountId = accountState.id;
        }

        const mainZkSyncContract = new Contract(
            ethProxy.contractAddress.mainContract,
            SYNC_MAIN_CONTRACT_INTERFACE,
            this.ethSigner
        );

        const tokenAddress = this.provider.tokenSet.resolveTokenAddress(
            withdraw.token
        );
        const ethTransaction = await mainZkSyncContract.fullExit(
            accountId,
            tokenAddress,
            {
                gasLimit: utils.bigNumberify("500000"),
                gasPrice,
                ...withdraw.ethTxOptions
            }
        );

        return new ETHOperation(ethTransaction, this.provider);
    }

    private async setRequiredAccountIdFromServer(actionName: string) {
        if (this.accountId === undefined) {
            const accountIdFromServer = await this.getAccountId();
            if (accountIdFromServer == null) {
                throw new Error(
                    `Failed to ${actionName}: Account does not exist in the zkSync network`
                );
            } else {
                this.accountId = accountIdFromServer;
            }
        }
    }
}

class ETHOperation {
    state: "Sent" | "Mined" | "Committed" | "Verified" | "Failed";
    error?: ZKSyncTxError;
    priorityOpId?: utils.BigNumber;

    constructor(
        public ethTx: ContractTransaction,
        public zkSyncProvider: Provider
    ) {
        this.state = "Sent";
    }

    async awaitEthereumTxCommit() {
        if (this.state != "Sent") return;

        const txReceipt = await this.ethTx.wait();
        for (const log of txReceipt.logs) {
            const priorityQueueLog = SYNC_MAIN_CONTRACT_INTERFACE.parseLog(log);
            if (priorityQueueLog && priorityQueueLog.values.serialId != null) {
                this.priorityOpId = priorityQueueLog.values.serialId;
            }
        }
        if (!this.priorityOpId) {
            throw new Error("Failed to parse tx logs");
        }

        this.state = "Mined";
        return txReceipt;
    }

    async awaitReceipt(): Promise<PriorityOperationReceipt> {
        this.throwErrorIfFailedState();

        await this.awaitEthereumTxCommit();
        if (this.state != "Mined") return;
        const receipt = await this.zkSyncProvider.notifyPriorityOp(
            this.priorityOpId.toNumber(),
            "COMMIT"
        );

        if (!receipt.executed) {
            this.setErrorState(
                new ZKSyncTxError("Priority operation failed", receipt)
            );
            this.throwErrorIfFailedState();
        }

        this.state = "Committed";
        return receipt;
    }

    async awaitVerifyReceipt(): Promise<PriorityOperationReceipt> {
        await this.awaitReceipt();
        if (this.state != "Committed") return;

        const receipt = await this.zkSyncProvider.notifyPriorityOp(
            this.priorityOpId.toNumber(),
            "VERIFY"
        );

        this.state = "Verified";

        return receipt;
    }

    private setErrorState(error: ZKSyncTxError) {
        this.state = "Failed";
        this.error = error;
    }

    private throwErrorIfFailedState() {
        if (this.state == "Failed") throw this.error;
    }
}

class Transaction {
    state: "Sent" | "Committed" | "Verified" | "Failed";
    error?: ZKSyncTxError;

    constructor(
        public txData,
        public txHash: string,
        public sidechainProvider: Provider
    ) {
        this.state = "Sent";
    }

    async awaitReceipt(): Promise<TransactionReceipt> {
        this.throwErrorIfFailedState();

        if (this.state !== "Sent") return;

        const receipt = await this.sidechainProvider.notifyTransaction(
            this.txHash,
            "COMMIT"
        );

        if (!receipt.success) {
            this.setErrorState(
                new ZKSyncTxError(
                    `ZKSync transaction failed: ${receipt.failReason}`,
                    receipt
                )
            );
            this.throwErrorIfFailedState();
        }

        this.state = "Committed";
        return receipt;
    }

    async awaitVerifyReceipt(): Promise<TransactionReceipt> {
        await this.awaitReceipt();
        const receipt = await this.sidechainProvider.notifyTransaction(
            this.txHash,
            "VERIFY"
        );

        this.state = "Verified";
        return receipt;
    }

    private setErrorState(error: ZKSyncTxError) {
        this.state = "Failed";
        this.error = error;
    }

    private throwErrorIfFailedState() {
        if (this.state == "Failed") throw this.error;
    }
}
