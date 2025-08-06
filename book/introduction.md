# Kabu Documentation
_Documentation for the Kabu MEV bot_

<div align="center">

<img src="images/kabu_logo.jpg" alt="Kabu Logo" width="300">

[![Telegram Chat][tg-badge]][tg-url]

</div>

## What is Kabu?

Kabu is a backrunning bot, currently under heavy development. It continues the journey of [loom](https://github.com/dexloom/loom). Since then many breaking changes have been made to revm, reth and alloy. The goal here is to make everything work again and modernize the codebase. Currently, Kabu is a work in progress and not yet ready for production use.

## Who is Kabu for?

For everyone that does not like to reinvent the wheel all the time. Have foundation to work with, extend it, rewrite it or use it as playground to learn about MEV and backrunning.

## Kabu is opinionated

- Kabu will only support exex and json-rpc
- We reuse as much as possible from reth, alloy and revm
- We keep as close as possible to the architecture of reth

## Why "Kabu"?

In Japanese, *kabu* (株) means "stock" — both in the financial sense and as a metaphor for growth.

## High level architecture
The kabu framework is using the alloy type system and has a deep integration with reth to receive events.

<div align="center">

![High level architecture](images/high_level_architecture.svg)

</div>


[tg-badge]: https://img.shields.io/badge/telegram-kabu-2C5E3D?style=plastic&logo=telegram
[tg-url]: https://t.me/joinkabu