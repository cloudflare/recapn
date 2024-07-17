#! /usr/bin/env bash

cd "$(dirname "$0")"

files=(
    "capnp/rpc.capnp"
)

capnp compile ${files[*]} -o- | cargo run -p recapnc --bin capnpc-rust