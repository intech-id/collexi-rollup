* First, deploy Token according to contracts/deploy-k8s.sh

* Open in a dedicated shell a port-forward to Postgres DB:  
`kubectl -n colexi-dev port-forward svc/postgres 5432:5432`

* Drop Postgres plasma database:  
`DROP DATABASE IF EXISTS plasma;`

* Select `dev` environment:  
`zksync env dev`

* Update `ERC721_ADDRESS` variable in `dev.env`

* Init DB :  
```
cd core/storage
f diesel database setup
f diesel migration run
cd ../../
```

* Build & deploy contracts :  
```
zksync build-contracts
f genesis.sh
f bin/deploy-contracts-custom.sh
```

* Generate and insert Postgres data :
```
CONTRACT_ADDR=$(f bash -c 'echo $CONTRACT_ADDR')
GOVERNANCE_ADDR=$(f bash -c 'echo $GOVERNANCE_ADDR')

echo "INSERT INTO server_config (contract_addr, gov_contract_addr) VALUES ('$CONTRACT_ADDR', '$GOVERNANCE_ADDR') ON CONFLICT (id) DO UPDATE SET (contract_addr, gov_contract_addr) = ('$CONTRACT_ADDR', '$GOVERNANCE_ADDR')"
echo "INSERT INTO eth_parameters (nonce, gas_price_limit, commit_ops, verify_ops, withdraw_ops) VALUES ('0', '400000000000', 0, 0, 0) ON CONFLICT (id) DO UPDATE SET (commit_ops, verify_ops, withdraw_ops) = (0, 0, 0)"
```

* Update `zksync/manifests/configmap.yml` and `rollup-ui/manifests/configmap.yml` with contract addresses