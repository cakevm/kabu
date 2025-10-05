## All targets
# Default target - show help
.PHONY: help
help:
	@echo "Kabu Makefile targets:"
	@echo ""
	@echo "Build targets:"
	@echo "  make build        - Build debug version"
	@echo "  make release      - Build release version"
	@echo "  make maxperf      - Build maximum performance version"
	@echo ""
	@echo "Documentation:"
	@echo "  make doc          - Build Rust documentation"
	@echo "  make book         - Build the book"
	@echo "  make test-book    - Test book examples"
	@echo "  make serve-book   - Serve book locally (http://localhost:3000)"
	@echo "  make clean-book   - Clean book build"
	@echo ""
	@echo "Development:"
	@echo "  make test         - Run all tests"
	@echo "  make bench        - Run benchmarks"
	@echo "  make fmt          - Format code"
	@echo "  make clippy       - Run linter"
	@echo "  make clippy-fix   - Auto-fix clippy warnings"
	@echo "  make fix          - Auto-fix compiler warnings"
	@echo "  make check        - Check for warnings"
	@echo "  make pre-release  - Run all checks before release"
	@echo "  make clean        - Clean all build artifacts"
	@echo ""
	@echo "Testing:"
	@echo "  make swap-test FILE=path/to/test.toml - Run specific swap test"
	@echo "  make swap-test-all                    - Run all swap tests (normal timeout)"
	@echo "  make swap-test-all-ci                 - Run all swap tests (CI timeout)"
	@echo "  make replayer                         - Run replayer test"

# Make help the default target
.DEFAULT_GOAL := help

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

# Build the book
.PHONY: book
book:
	@echo "Building book..."
	@if ! command -v mdbook &> /dev/null; then \
		echo "mdbook not found. Installing..."; \
		cargo install mdbook; \
	fi
	@if ! command -v mdbook-mermaid &> /dev/null; then \
		echo "mdbook-mermaid not found. Installing..."; \
		cargo install mdbook-mermaid; \
	fi
	mdbook build

# Test the book
.PHONY: test-book
test-book:
	@echo "Testing book..."
	@if ! command -v mdbook &> /dev/null; then \
		echo "mdbook not found. Installing..."; \
		cargo install mdbook; \
	fi
	mdbook test

# Serve the book locally
.PHONY: serve-book
serve-book:
	@echo "Serving book at http://localhost:3000"
	@if ! command -v mdbook &> /dev/null; then \
		echo "mdbook not found. Installing..."; \
		cargo install mdbook; \
	fi
	mdbook serve --open

# Clean the book build
.PHONY: clean-book
clean-book:
	@echo "Cleaning book build..."
	rm -rf target/book

## Development commands
# Target to run all tests using nextest
.PHONY: test
test:
	@command -v cargo-nextest >/dev/null 2>&1 || cargo install cargo-nextest --locked
	cargo nextest run --all-features --workspace --bins --lib --tests

# Target to run tests using standard cargo test
.PHONY: test-cargo
test-cargo:
	cargo test --all --all-features --workspace --lib --bins --tests

# Target to run doc tests only
.PHONY: test-doc
test-doc:
	cargo test --doc --all-features

# Target to clean build artifacts
.PHONY: clean
clean:
	cargo clean
	rm -rf target/book

# Target to run all benchmarks
.PHONY: bench
bench:
	cargo bench

# Target to run cargo clippy
.PHONY: clippy
clippy:
	cargo clippy --all-targets --all-features --tests --benches -- -D warnings

# Target to automatically fix clippy warnings
.PHONY: clippy-fix
clippy-fix:
	cargo clippy --all-targets --all-features --tests --benches --fix --allow-dirty --allow-staged

# Target to automatically fix compiler warnings
.PHONY: fix
fix:
	cargo fix --all-targets --all-features --allow-dirty --allow-staged

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
	make test
	make test-doc

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
	@echo "Running specific swap test: $(FILE)\n"
	@RUST_LOG=${RL:-info} cargo run --package kabu-backtest-runner --bin kabu-backtest-runner -- \
		--timeout 10 --wait-init 1 $(FILE); \
	EXIT_CODE=$$?; \
	if [ $$EXIT_CODE -ne 0 ]; then \
		echo "\n\033[0;31mError: Swap test failed with code $$EXIT_CODE\033[0m\n"; \
		exit 1; \
	else \
		echo "\n\033[0;32mSwap test completed successfully.\033[0m"; \
	fi

# Run all swap tests
.PHONY: swap-test-all
swap-test-all:
	@echo "Running all swap tests in ./testing/backtest-runner/\n"
	@RUST_LOG=${RL:-off} cargo run --package kabu-backtest-runner --bin kabu-backtest-runner -- \
		--timeout 10 --wait-init 1 --flashbots-wait 0 ./testing/backtest-runner/; \
	EXIT_CODE=$$?; \
	if [ $$EXIT_CODE -ne 0 ]; then \
		echo "\n\033[0;31mError: Swap test failed with code $$EXIT_CODE\033[0m\n"; \
		exit 1; \
	else \
		echo "\n\033[0;32mAll swap tests completed successfully.\033[0m"; \
	fi

# Run all swap tests with CI timeouts (longer timeouts for slower CI environments)
.PHONY: swap-test-all-ci
swap-test-all-ci:
	@echo "Running all swap tests with CI timeouts\n"
	@RUST_LOG=off cargo run --package kabu-backtest-runner --bin kabu-backtest-runner -- \
		--timeout 60 --wait-init 10 --flashbots-wait 15 ./testing/backtest-runner/; \
	EXIT_CODE=$$?; \
	if [ $$EXIT_CODE -ne 0 ]; then \
		echo "\n\033[0;31mError: CI swap test failed with code $$EXIT_CODE\033[0m\n"; \
		exit 1; \
	else \
		echo "\n\033[0;32mCI swap test completed successfully.\033[0m"; \
	fi


