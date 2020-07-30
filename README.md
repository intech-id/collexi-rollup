# PoC for ZK Rollup on Colexi ERC-721

Forked from [https://github.com/matter-labs/zksync](https://github.com/matter-labs/zksync)

## Run locally

* Start local chain & postgres db

```
# in `infra` repo:
cd local
docker-compose up -d
```

* Deploy ERC-721 contract

```
# in `contracts` repo:
yarn deploy:local
```

* Deploy ZK contracts & init db

```
# in `zksync` repo:
bash ./reset-rollup.sh
```

* Start ZK Server

```
f cargo run --bin server
```

* Start Dummy prover

```
f cargo run --bin dummy_prover local
```