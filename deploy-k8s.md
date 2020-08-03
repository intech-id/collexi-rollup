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
f bin/deploy-contracts-custom.sh
f genesis.sh
```

* Generate and insert Postgres data :
```
zksync db-insert-contract
zksync db-insert-eth-data
```

* Update `zksync/manifests/configmap.yml` and `rollup-ui/manifests/configmap.yml` with contract addresses