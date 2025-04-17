#! /usr/bin/env bash

cd "$(dirname "$0")"

files=(
    "capnp/test.capnp"
    "capnp/test-import.capnp"
    "capnp/test-import2.capnp"
)

capnp compile ${files[*]} -I. --no-standard-import -o- | cargo run -p recapnc --bin capnpc-rust