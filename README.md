# Snap Coin Utils
## Installation
To install Snap Coin Utils, run:
```bash
cargo install snap-coin-utils
```
Make sure you have cargo, and rust installed.

## General Information
This package is a helper CLI for accessing blockchain 

## Usage
Usage:
```bash
snap-coin-node <NODE> <COMMAND>

Commands:
  block       Get block by height or hash
  tx          Get transaction by hash (base36)
  addr        Get address (base36) info
  height      Get current blockchain height
  difficulty  Get current difficulty
  help        Print this message or the help of the given subcommand(s)

Arguments:
  <NODE>  Node address to connect too

Options:
  -h, --help     Print help
  -V, --version  Print version
```