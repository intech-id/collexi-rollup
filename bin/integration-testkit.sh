#!/bin/bash
. .setup_env

set -e

trap clean_up EXIT

PREV_WEB3_URL=$WEB3_URL

function clean_up() {
    exitcode=$?
    if [[ $ZKSYNC_ENV == dev ]]; then
        docker kill $CONTAINER_ID > /dev/null;
        if [[ $? != 0 && $CONTAINER_ID != '' ]]; then
            echo "problem killing $CONTAINER_ID"
        fi
    fi
    export WEB3_URL=$PREV_WEB3_URL
    exit $exitcode
}

# set up fast geth
if [[ $ZKSYNC_ENV == ci ]]; then
    export WEB3_URL=http://geth-fast:8545
elif [[ $ZKSYNC_ENV == dev ]]; then
    CONTAINER_ID=$(docker run --rm -d -p 7545:8545 matterlabs/geth:latest fast)
    export WEB3_URL=http://localhost:7545
fi

export ETH_NETWORK="test"
make build-contracts

cargo run --bin testkit --release
cargo run --bin migration_test --release
cargo run --bin exodus_test --release
