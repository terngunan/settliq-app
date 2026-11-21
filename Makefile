CONTRACTS_DIR := settliq_contracts
BACKEND_DIR := backend

.PHONY: build build-optimised check-wasm-size test backend-install backend-build backend-test test-all

build:
	cd $(CONTRACTS_DIR) && cargo build --target wasm32-unknown-unknown --release

build-optimised:
	cd $(CONTRACTS_DIR) && cargo build --target wasm32-unknown-unknown --release
	cd $(CONTRACTS_DIR) && wasm-opt -Oz -o target/wasm32-unknown-unknown/release/settliq_contracts.optimized.wasm target/wasm32-unknown-unknown/release/settliq_contracts.wasm

check-wasm-size:
	@ls -lh $(CONTRACTS_DIR)/target/wasm32-unknown-unknown/release/*.wasm

backend-install:
	cd $(BACKEND_DIR) && pnpm install

backend-build:
	cd $(BACKEND_DIR) && pnpm build

backend-test:
	cd $(BACKEND_DIR) && pnpm test

test:
	cd $(CONTRACTS_DIR) && cargo test

test-all: test backend-test
