FROM rust:1.44 AS builder

ARG zk_env="dev"

WORKDIR /build

RUN rustup target add x86_64-unknown-linux-musl
RUN apt-get update && apt-get install -y musl-tools libssl-dev
RUN ln /usr/include/x86_64-linux-gnu/openssl/opensslconf.h /usr/include/openssl/

ENV ZKSYNC_HOME=/build
ENV PATH="$PATH:${ZKSYNC_HOME}/bin"

ENV RUSTFLAGS=-Clinker=musl-gcc
ENV PKG_CONFIG_ALLOW_CROSS=1
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl

COPY . /build

RUN echo "${zk_env}" > /build/etc/env/current

RUN f cargo build --release --target=x86_64-unknown-linux-musl -p server -p prover

FROM alpine:latest

RUN apk add libpq

WORKDIR /zksync
RUN mkdir -p /zksync/contracts/build

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/server /usr/local/bin/server
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/dummy_prover /usr/local/bin/dummy_prover
COPY --from=builder /build/contracts/build/ZkSync.json /zksync/contracts/build
COPY --from=builder /build/contracts/build/Governance.json /zksync/contracts/build
COPY --from=builder /build/contracts/build/IERC20.json /zksync/contracts/build
COPY --from=builder /build/contracts/build/IEIP1271.json /zksync/contracts/build
