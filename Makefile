## All targets
# Target to build the project
.PHONY: build
build:
	cargo build --all

# Build release
.PHONY: release
release:
	RUSTFLAGS="-D warnings -C target-cpu=native" cargo build --release

# Build optimized release
.PHONY: maxperf
maxperf:
	RUSTFLAGS="-D warnings -C target-cpu=native" cargo build --profile maxperf

# Build docs
.PHONY: doc
doc:
	RUSTDOCFLAGS="--show-type-layout --generate-link-to-definition --enable-index-page -D warnings -Z unstable-options" \
	cargo +nightly doc --workspace --all-features --no-deps --document-private-items --exclude kabu-defi-abi

## Development commands
# Target to run all tests (excluding loom-defi-abi doc tests and flashbots env-dependent tests)
.PHONY: test
test:
	cargo test --all --all-features --workspace --exclude kabu-defi-abi --lib --bins --tests -- --skip test_send_bundle --skip test_client_send_bundle
	cargo test --all --all-features --workspace --exclude kabu-defi-abi --doc

# Target to run all benchmarks
.PHONY: clean
clean:
	cargo clean

# Target to run all benchmarks
.PHONY: bench
bench:
	cargo bench

# Target to run cargo clippy
.PHONY: clippy
clippy:
	cargo clippy --all-targets --all-features --tests --benches -- -D warnings

# format kabu
.PHONY: fmt
fmt:
	cargo +stable fmt --all

# check files format fmt
.PHONY: fmt-check
fmt-check:
	cargo +stable fmt --all --check

# check for warnings in tests
.PHONY: check
check:
	cargo check --all-targets --all-features --tests --benches

# format toml
.PHONY: taplo
taplo:
	taplo format

# check files format with taplo
.PHONY: taplo-check
taplo-check:
	taplo format --check

# check licences
.PHONY: deny-check
deny-check:
	cargo deny --all-features check

# check for unused dependencies
.PHONY: udeps
udeps:
	cargo install cargo-machete --locked && cargo-machete --with-metadata

# check files format with fmt and clippy
.PHONY: pre-release
pre-release:
	make fmt
	make check
	make clippy
	make taplo
	make udeps

# replayer test
.PHONY: replayer
replayer:
	@echo "Running Replayer test case: $(FILE)\n"
	@RL=${RL:-info}; \
	RUST_LOG=$(RL) cargo run --package kabu-replayer --bin kabu-replayer -- --terminate-after-block-count 10; \
	EXIT_CODE=$$?; \
	if [ $$EXIT_CODE -ne 0 ]; then \
		echo "\n\033[0;31mError: Replayer tester exited with code $$EXIT_CODE\033[0m\n"; \
	else \
		echo "\n\033[0;32mReplayer test passed successfully.\033[0m"; \
	fi

# swap tests with kabu_anvil
.PHONY: swap-test
swap-test:
	@echo "Running anvil swap test case: $(FILE)\n"
	@RL=${RL:-info}; \
    RUST_LOG=$(RL) cargo run --package kabu-backtest-runner --bin kabu-backtest-runner -- --config $(FILE) --timeout 40 --wait-init 7; \
	EXIT_CODE=$$?; \
	if [ $$EXIT_CODE -ne 0 ]; then \
		echo "\n\033[0;31mError: Anvil swap tester exited with code $$EXIT_CODE\033[0m\n"; \
		exit 1; \
	else \
		echo "\n\033[0;32mAnvil swap test passed successfully.\033[0m"; \
	fi

.PHONY: swap-test-1
swap-test-1: FILE="./testing/backtest-runner/test_18498188.toml"
swap-test-1: swap-test

.PHONY: swap-test-2
swap-test-2: FILE="./testing/backtest-runner/test_18567709.toml"
swap-test-2: swap-test

.PHONY: swap-test-3
swap-test-3: FILE="./testing/backtest-runner/test_19101578.toml"
swap-test-3: swap-test

.PHONY: swap-test-4
swap-test-4:FILE="./testing/backtest-runner/test_19109955.toml"
swap-test-4: swap-test

.PHONY: swap-test-5
swap-test-5:FILE="./testing/backtest-runner/test_20927846.toml"
swap-test-5: swap-test

.PHONY: swap-test-6
swap-test-6:FILE="./testing/backtest-runner/test_20935488.toml"
swap-test-6: swap-test

#.PHONY: swap-test-7
#swap-test-7:FILE="./testing/backtest-runner/test_20937428.toml"
#swap-test-7: swap-test

.PHONY: swap-test-8
swap-test-8:FILE="./testing/backtest-runner/test_21035613.toml"
swap-test-8: swap-test

.PHONY: swap-test-all
swap-test-all: RL=off
swap-test-all:
	@$(MAKE) swap-test-1 RL=$(RL)
	@$(MAKE) swap-test-2 RL=$(RL)
	@$(MAKE) swap-test-3 RL=$(RL)
	@$(MAKE) swap-test-4 RL=$(RL)
	@$(MAKE) swap-test-5 RL=$(RL)
	@$(MAKE) swap-test-6 RL=$(RL)
	@$(MAKE) swap-test-8 RL=$(RL)


