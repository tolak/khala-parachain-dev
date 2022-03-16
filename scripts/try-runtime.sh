#!/bin/bash

# SKIP_WASM_BUILD=1
cargo run --release --features try-runtime \
  -- try-runtime \
  -d /tmp/blackhole \
  --chain khala-staging-2004 \
  on-runtime-upgrade live \
  -u ws://127.0.0.1:9944 \
  --at 0xfa5b382cc71582ade9691a3e86a59c147fb6dd6a587a9001c075f3f4914c21e9 \
  |& tee ./tmp/sim.log

# -l trace,soketto=warn,jsonrpsee_ws_client=warn,remote-ext=warn,trie=warn,wasmtime_cranelift=warn \
