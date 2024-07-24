#!/bin/bash
set -e
cd "$(dirname "$0")"
args=("$@")

if (( ${#args[@]} < 1 )); then
    echo "$0 {TARGET} {OPTIONS}"
    echo "See 'cargo fuzz run --help' for options"
    exit
fi

if [[ -n "corpus/$1" ]]; then
    mkdir --parents "corpus/$1"
fi

if [[ -n "samples/$1" ]]; then
    mkdir --parents "samples/$1"
fi

cargo run -p fuzz --bin generate-samples -- "./samples/"
cargo fuzz run $1 "corpus/$1" "samples/$1" "${args[@]:1}"