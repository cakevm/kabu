# Getting started

## Checkout the repository
Clone the repository:
```sh
git clone git@github.com:cakevm/kabu.git
```

## Setting up topology
Copy `config-example.toml` to `config.toml` and configure according to your setup.

## Updating private key encryption password
Private key encryption password is individual secret key that is generated automatically but can be replaced

It is located in ./crates/defi-entities/private.rs and looks like

```rust
pub const KEY_ENCRYPTION_PWD: [u8; 16] = [35, 48, 129, 101, 133, 220, 104, 197, 183, 159, 203, 89, 168, 201, 91, 130];
```

To change key encryption password run and replace content of KEY_ENCRYPTION_PWD

```sh
cargo run --bin keys generate-password  
```

To get encrypted key run:

```sh
cargo run --bin keys encrypt --key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
```

## Setup database
Install postgresql and create database and user.

Create user and db:
```shell
su - postgres
createuser kabu
createdb kabu
```

Run `psql` and update user and privileges:
```psql
alter user kabu with encrypted password 'kabu';
grant all privileges on database kabu to kabu;
create schema kabu;
grant usage on schema kabu to kabu;
grant create on schema kabu to kabu;
\q
```

## Starting kabu
```sh
DATA=<ENCRYPTED_PRIVATE_KEY> cargo run --bin kabu
```

## Makefile
Makefile is shipped with following important commands:

- build - builds all binaries
- fmt - formats kabu with rustfmt
- pre-release - check code with rustfmt and clippy
- clippy - check code with clippy

## Testing
Testing kabu requires two environment variables pointing at archive node:

- MAINNET_WS - websocket url of archive node
- MAINNET_HTTP - http url of archive node

To run tests:

```shell
make test
```