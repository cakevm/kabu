# Getting Started

## Prerequisites

- Rust (latest stable)
- Optional: PostgreSQL (for database)
- Optional: InfluxDB (for metrics)
- RPC node (e.g., your own node, Alchemy, Infura)

## Building Kabu

### 1. Clone the repository

```bash
git clone https://github.com/cakevm/kabu.git
cd kabu
```

### 2. Build the project

```bash
# Development build
make

# Release build (optimized)
make release

# Maximum performance build
make maxperf
```

## Configuration

### 1. Create configuration file

```bash
cp config.example.toml config.toml
```

### 2. Edit `config.toml`

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

### 3. Set up environment variables

Create a `.env` file:

```bash
# .env file
DATABASE_URL=postgresql://kabu:kabu@localhost/kabu
MAINNET_WS=wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
DATA=your_encrypted_private_key  # Optional: for signing
```

## Database Setup (Optional)

### 1. Create the database and user

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

### 2. Set environment variables

```bash
# Create .env file
echo "DATABASE_URL=postgresql://kabu:kabu@localhost/kabu" >> .env
```

### 3. Run migrations

Migrations will run automatically on first startup, or manually:

```bash
diesel migration run --database-url postgresql://kabu:kabu@localhost/kabu
```

## Running Kabu

### As a standalone application

```bash
# Run with remote node
cargo run --bin kabu -- remote --kabu-config config.toml

# Run with local Reth node
cargo run --bin kabu -- node --kabu-config config.toml
```

### As a Reth ExEx (Execution Extension)

```bash
# Start Reth with Kabu ExEx
reth node \
  --chain mainnet \
  --datadir ./reth-data \
  --kabu-config config.toml
```

### For testing with backtest runner

```bash
# Run specific test
cargo run --bin kabu-backtest-runner -- \
  --config ./testing/backtest-runner/test_18567709.toml

# Run all tests
make swap-test-all
```

## Development Tools

### Code Quality

```bash
# Format code
make fmt

# Run linter
make clippy

# Run tests
make test

# Pre-release checks (fmt, clippy, taplo, udeps)
make pre-release

# Clean unused dependencies
make udeps
```

### Documentation

```bash
# Build the book
make book

# Test book examples
make test-book

# Serve book locally (opens at http://localhost:3000)
make serve-book

# Build API documentation
make doc
```

## Multicaller Contract

Kabu uses a custom multicaller contract for efficient swap execution. The contract needs to be deployed to your target network. Find the contract at [kabu-contract](https://github.com/cakevm/kabu-contract).

## Private Key Management

### Generate encryption password

```bash
cargo run --bin keys generate-password
```

Replace the generated password in `./crates/defi-entities/private.rs`:

```rust,ignore
pub const KEY_ENCRYPTION_PWD: [u8; 16] = [/* your generated password */];
```

### Encrypt your private key

```bash
cargo run --bin keys encrypt --key 0xYOUR_PRIVATE_KEY
```

Use the encrypted key in the `DATA` environment variable when running Kabu.

## Quick Start Example

1. Clone and build:
   ```bash
   git clone https://github.com/cakevm/kabu.git
   cd kabu
   make release
   ```

2. Configure:
   ```bash
   cp config.example.toml config.toml
   # Edit config.toml with your settings
   ```

3. Run:
   ```bash
   cargo run --bin kabu -- remote --kabu-config config.toml
   ```

## Troubleshooting

- **Connection issues**: Ensure your RPC endpoint is accessible and supports WebSocket
- **Database errors**: Check PostgreSQL is running and credentials are correct
- **Build failures**: Run `make clean` and rebuild
- **Test failures**: Ensure you have proper archive node access via environment variables