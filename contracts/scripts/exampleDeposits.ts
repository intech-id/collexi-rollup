import {Contract, ethers} from "ethers";
import {parseEther} from "ethers/utils";
import {Deployer} from "../src.ts/deploy";

const provider = new ethers.providers.JsonRpcProvider(process.env.WEB3_URL);
const wallet = ethers.Wallet.fromMnemonic(process.env.MNEMONIC, "m/44'/60'/0'/0/1").connect(provider);
const franklinAddress = "010203040506070809101112131415161718192021222334252627";
const franklinAddressBinary = Buffer.from(franklinAddress, "hex");

async function main() {
    const deployer = new Deployer(wallet, false);
    const franklinDeployedContract = deployer.getDeployedProxyContract("Franklin");
    const depositValue = parseEther("0.3");
    const tx = await franklinDeployedContract.depositETH(franklinAddressBinary, {value: depositValue});
    const receipt = await tx.wait();
    console.log(receipt);
}

main();
