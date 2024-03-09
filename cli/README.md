# snarkos-cli

[![Crates.io](https://img.shields.io/crates/v/snarkos-cli.svg?color=neon)](https://crates.io/crates/snarkos-cli)
[![Authors](https://img.shields.io/badge/authors-Aleo-orange.svg)](https://aleo.org)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](./LICENSE.md)

The `snarkos-cli` crate provides the `CLI` struct, which is responsible for
providing a command-line interface to the node.

# MONADIC.US genesis block generation

The command `snarkos genesis` can be used to generate genesis blocks ahead of
time.

The following flags are supported:

- `--genesis-key <key>` (optional): pass in an existing private key used to make
  the genesis block. If not passed, one is generated for you (and printed at the
  end of the command).
- `--output <filename>` (defaults to `./genesis.block`): where to write the
  genesis block to
- `--committee-size <size>` (defaults to 4): how large the committee should be.
  Ignored if `--bonded-balances` is set.
- `--seed <seed>` (defaults to `1234567890`): the seed to use when generating
  the committee private keys and genesis private key.
- `--bonded-balances <balances>` (optional): a JSON object from address to
  balance to use for bonded balances. If not passed, these keys are generated
  and printed to the console at the end of the command.
- `--output-committee <filename>` (optional): a destination path for the JSON
  file that represents the committee addresses/private keys if they were
  generated.

## Example

Use `snarkos genesis --output genesis.block --output-committee committee.json`
to generate a genesis block.

The genesis block will be written to `genesis.block` and the committee (with
addresses and private keys) will be written to `committee.json`, as well as in
the console stdout.

## Usage in nodes

Use `snarkos start --genesis genesis.block` to specify to the node starter where
the genesis block is.

### Bootstrap peers

Use `snarkos start --bootstrap-peers bootstrap-peers.json` to specify to the
node starter what the trusted bootstrap peers are.

The bootstrap peers file (`bootstrap-peers.json`) should be of the form:

```json
[
  "64.23.169.88:4130",
  "146.190.35.174:4130",
  "45.55.201.67:4130",
  "45.55.201.80:4130"
]
```

## Other changes

**NOTE:** `snarkos start --peers` was changed to `snarkos start --trusted-peers`
to distinguish it from `snarkos start --bootstrap-peers`.
