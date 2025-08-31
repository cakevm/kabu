# Kabu

<div align="center">

<img src=".github/assets/kabu_logo.jpg" alt="Kabu Logo" width="300">

[![CI status](https://github.com/cakevm/kabu/actions/workflows/ci.yml/badge.svg?branch=main)][gh-kabu]
[![Book status](https://github.com/cakevm/kabu/actions/workflows/book.yml/badge.svg?branch=main)][gh-book]
[![Telegram Chat][tg-badge]][tg-url]

| [User Book](https://cakevm.github.io/kabu/)
| [Crate Docs](https://cakevm.github.io/kabu/docs/) |

[gh-kabu]: https://github.com/cakevm/kabu/actions/workflows/ci.yml
[gh-book]: https://github.com/cakevm/kabu/actions/workflows/book.yml
[tg-badge]: https://img.shields.io/badge/telegram-kabu-2C5E3D?style=plastic&logo=telegram
[tg-url]: https://t.me/joinkabu

</div>

> ⚠️ **WARNING: This codebase is under heavy development and far from ready for production use.** 
> 
> The code is currently being actively refactored and modernized. Many features are incomplete, broken, or subject to breaking changes without notice. It is a long road from custom types to reth node types and using all providers from reth. With limited time, it will take a while. In the meantime, feel free to join the [Telegram group](https://t.me/joinkabu) for discussion.

## What is Kabu?

Kabu is a backrunning bot, currently under heavy development. It continues the journey of [loom](https://github.com/dexloom/loom). Since then many breaking changes have been made to revm, reth and alloy. The goal here is to make everything work again and modernize the codebase. Currently, Kabu is a work in progress and not yet ready for production use.

## Who is Kabu for?

For everyone that does not like to reinvent the wheel all the time. Have foundation to work with, extend it, rewrite it or use it as playground to learn about MEV and backrunning.


## Kabu is opinionated
- Kabu will only support exex and json-rpc.
- We reuse as much as possible from reth, alloy and revm
- We keep as close as possible to the architecture of reth

## Roadmap
- Remove `KabuDataTypes`
- Remove `Actor` model and use trait based components like in reth
- Remove topology and simplify the config / codebase
- Refactor the extra db pool cache layer to make it optional
- Have components that can be replaced with custom implementations

## Kabu contract
Find the Kabu contract [here](https://github.com/cakevm/kabu-contract).

## Why "Kabu"?

In Japanese, *kabu* (株) means "stock" — both in the financial sense and as a metaphor for growth.

## Setup

### Prerequisites

- Rust
- Optional: PostgreSQL (for database)
- Optional: InfluxDB (for metrics)
- RPC node (e.g. your own node)

### Building Kabu

1. **Clone the repository:**
   ```bash
   git clone https://github.com/cakevm/kabu.git
   cd kabu
   ```

2. **Build the project:**
   ```bash
   # Development build
   make
   
   # Release build (optimized)
   make release
   
   # Maximum performance build
   make maxperf
   ```

### Configuration

1. **Create configuration file:**
   ```bash
   cp config.example.toml config.toml
   ```

2. **Edit `config.toml`:**
   ```toml
   [clients.local]
   url = "ws://localhost:8545"  # Your node endpoint
   
   [database]
   url = "postgresql://kabu:kabu@localhost/kabu"
   
   [actors.signers]
   # Add your signer configuration
   
   [actors.broadcaster]
   flashbots_signer = "0x..."  # Your flashbots signer
   ```

3. **Set up environment variables:**
   ```bash
   # .env file
   DATABASE_URL=postgresql://kabu:kabu@localhost/kabu
   MAINNET_WS=wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
   DATA=your_encrypted_private_key  # Optional: for signing
   ```

### Database Setup (optional)

1. **Create the database and user:**
   ```sql
   sudo -u postgres psql
   CREATE DATABASE kabu;
   CREATE USER kabu WITH PASSWORD 'kabu';
   
   \c kabu;
   CREATE SCHEMA kabu AUTHORIZATION kabu;
   GRANT ALL PRIVILEGES ON DATABASE kabu TO kabu;
   ALTER ROLE kabu SET search_path TO kabu, public;
   \q
   ```

2. **Set environment variables:**
   ```bash
   # Create .env file
   echo "DATABASE_URL=postgresql://kabu:kabu@localhost/kabu" >> .env
   ```

3. **Run migrations:**
   ```bash
   # Migrations will run automatically on first startup
   # Or manually run:
   diesel migration run --database-url postgresql://kabu:kabu@localhost/kabu
   ```

### Running Kabu

#### As a standalone application:
```bash
# Run with remote node
cargo run --bin kabu -- remote --kabu-config config.toml

# Run with local Reth node
cargo run --bin kabu -- node --kabu-config config.toml
```

#### As a Reth ExEx (Execution Extension):
```bash
# Start Reth with Kabu ExEx
reth node \
  --chain mainnet \
  --datadir ./reth-data \
  --kabu-config config.toml
```

#### For testing with backtest runner:
```bash
# Run specific test
cargo run --bin kabu-backtest-runner -- \
  --config ./testing/backtest-runner/test_18567709.toml

# Run all tests
make swap-test-all
```

### Development Tools

1. **Format code:**
   ```bash
   make fmt
   ```

2. **Run linter:**
   ```bash
   make clippy
   ```

3. **Run tests:**
   ```bash
   make test
   ```

4. **Pre-release checks:**
   ```bash
   make pre-release
   ```

5. **Clean unused dependencies:**
   ```bash
   make udeps
   ```

### Documentation

1. **Build the book:**
   ```bash
   make book
   ```

2. **Test book examples:**
   ```bash
   make test-book
   ```

3. **Serve book locally:**
   ```bash
   make serve-book  # Opens at http://localhost:3000
   ```

4. **Build API documentation:**
   ```bash
   make doc
   ```

## Acknowledgements

Many thanks to [dexloom](https://github.com/dexloom)! This project is a hard-fork from [loom](https://github.com/dexloom/loom), based on this [branch](https://github.com/dexloom/loom/tree/entityid). The `flashbots` crate is fork of [ethers-flashbots](https://github.com/onbjerg/ethers-flashbots). The `uniswap-v3-math` crate is a fork of [uniswap-v3-math](https://github.com/0xKitsune/uniswap-v3-math). Additionally, some code for the Uniswap V3 pools is derived from [amms-rs](https://github.com/darkforestry/amms-rs). Last but not least, a big shoutout to [Paradigm](https://github.com/paradigmxyz) — without their work, this project would not have been possible.

## License
This project is licensed under the [Apache 2.0](./LICENSE-APACHE) or [MIT](./LICENSE-MIT). 