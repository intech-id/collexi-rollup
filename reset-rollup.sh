#!/bin/bash

POSTGRES_CONTAINER="local_postgres_1"

# Clean DB, if required
docker exec -ti $POSTGRES_CONTAINER psql postgres://postgres@localhost/ "-c DROP DATABASE IF EXISTS plasma;"

# Init DB
cd core/storage
f diesel database setup
f diesel migration run
cd ../../

# Init chain
zksync build-contracts
zksync deploy-contracts

CONTRACT_ADDR=$(f bash -c 'echo $CONTRACT_ADDR')
GOVERNANCE_ADDR=$(f bash -c 'echo $GOVERNANCE_ADDR')

# Update DB
docker exec -ti $POSTGRES_CONTAINER psql postgres://postgres@localhost/plasma "-c INSERT INTO server_config (contract_addr, gov_contract_addr) VALUES ('$CONTRACT_ADDR', '$GOVERNANCE_ADDR') ON CONFLICT (id) DO UPDATE SET (contract_addr, gov_contract_addr) = ('$CONTRACT_ADDR', '$GOVERNANCE_ADDR')"
docker exec -ti $POSTGRES_CONTAINER psql postgres://postgres@localhost/plasma "-c INSERT INTO eth_parameters (nonce, gas_price_limit, commit_ops, verify_ops, withdraw_ops) VALUES ('0', '400000000000', 0, 0, 0) ON CONFLICT (id) DO UPDATE SET (commit_ops, verify_ops, withdraw_ops) = (0, 0, 0)"
f ./bin/genesis.sh
