# Kabu

<div align="center">

[![CI status](https://github.com/cakevm/kabu/actions/workflows/ci.yml/badge.svg?branch=main)][gh-kabu]
[![Book status](https://github.com/cakevm/kabu/actions/workflows/book.yml/badge.svg?branch=main)][gh-book]
[![Telegram Chat][tg-badge]][tg-url]

| [User Book](https://cakevm.github.io/kabu/)
| [Crate Docs](https://cakevm.github.io/kabu/docs/) |

[gh-kabu]: https://github.com/cakevm/kabu/actions/workflows/ci.yml
[gh-book]: https://github.com/cakevm/kabu/actions/workflows/book.yml
[tg-badge]: https://img.shields.io/badge/telegram-kabu-2CA5E0?style=plastic&logo=telegram
[tg-url]: https://t.me/joinkabu

</div>

## What is kabu?

kabu is a MEV bot framework, currently under heavy development.

## Kabu is opinionated
- Kabu will only support exex and json-rpc.
- We reuse as much as possible from reth, alloy and revm
- We keep as close as possible to the architecture of reth


## Acknowledgements

Many thanks to [Loom](https://github.com/dexloom/loom)! This project is a hard-fork from `loom`, based on this [branch](https://github.com/dexloom/loom/tree/entityid). The `flashbots` crate is fork of [ethers-flashbots](https://github.com/onbjerg/ethers-flashbots). The `uniswap-v3-math` crate is a fork of [uniswap-v3-math](https://github.com/0xKitsune/uniswap-v3-math). Additionally, some code for the Uniswap V3 pools is derived from [amms-rs](https://github.com/darkforestry/amms-rs). Last but not least, a big shoutout to [Paradigm](https://github.com/paradigmxyz) — without their work, this project would not have been possible.

## Licence

Licenced under the [MIT Licence](./LICENCE)
