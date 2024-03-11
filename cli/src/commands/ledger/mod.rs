// Copyright (C) 2019-2023 Aleo Systems Inc.
// This file is part of the snarkOS library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{fs, path::PathBuf, str::FromStr};

use anyhow::{ensure, Result};
use clap::{Args, Subcommand};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use serde::Deserialize;
use snarkvm::{
    circuit::AleoV0,
    console::{account::PrivateKey, network::MainnetV0, types::Address},
    ledger::{store::helpers::rocksdb::ConsensusDB, Transaction},
    synthesizer::VM,
};

mod util;

type Network = MainnetV0;
type Db = ConsensusDB<Network>;

#[derive(Debug, Args)]
pub struct Command {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TxOperation {
    from: PrivateKey<Network>,
    to: Address<Network>,
    amount: u32,
}

impl FromStr for TxOperation {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long)]
        genesis: PathBuf,
        /// A destination path for the ledger directory.
        #[arg(required = true, short, long)]
        output: PathBuf,
    },
    Tx {
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        #[arg(required = true, long)]
        operations: Vec<TxOperation>,
        #[arg(required = true, short, long)]
        output: PathBuf,
    },
    Add {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// A destination path for the ledger directory.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        /// The private key to use when generating the block.
        #[arg(name = "private-key", long)]
        private_key: PrivateKey<Network>,
        /// The number of transactions to add per block.
        #[arg(name = "txns-per-block", long)]
        txns_per_block: Option<usize>,
        /// The transactions file to read from. Should have been generated with `snarkos ledger tx`.
        #[arg(name = "txns-file")]
        txns_file: PathBuf,
    },
    View {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// The ledger from which to view a block.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        /// The block height to view.
        block_height: u32,
    },
    ViewAccountBalance {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// The ledger from which to view a block.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        /// The address's balance to view.
        address: String,
    },
}

impl Commands {
    pub fn parse(self) -> Result<String> {
        match self {
            Commands::Init { genesis, output } => {
                let ledger = util::open_ledger::<Network, Db>(genesis, output)?;
                let genesis_block = ledger.get_block(0)?;

                Ok(format!("Ledger written, genesis block hash: {}", genesis_block.hash()))
            }

            Commands::Tx { genesis, ledger, operations, output } => {
                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                let txns = operations
                    .into_iter()
                    .map(|op| {
                        util::make_transaction_proof::<_, _, AleoV0>(ledger.vm(), op.to, op.amount, op.from, None)
                    })
                    .collect::<Result<Vec<_>>>()?;

                util::write_json_to(&output, &txns)?;

                Ok(format!("Wrote {} transactions to {}.", txns.len(), output.display()))
            }

            Commands::Add { genesis, ledger, private_key, txns_per_block, txns_file } => {
                let mut rng = ChaChaRng::from_entropy();

                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                // Ensure we aren't trying to stick too many transactions into a block
                let per_block_max = VM::<Network, Db>::MAXIMUM_CONFIRMED_TRANSACTIONS;
                let per_block = txns_per_block.unwrap_or(per_block_max);
                ensure!(per_block <= per_block_max, "too many transactions per block (max is {})", per_block_max);

                // Load the transactions
                let txns: Vec<Transaction<MainnetV0>> = serde_json::from_reader(fs::File::open(txns_file)?)?;

                // Add the appropriate number of blocks
                let block_count = util::add_transaction_blocks(&ledger, private_key, &txns, per_block, &mut rng)?;

                Ok(format!("Inserted {block_count} blocks into the ledger."))
            }

            Commands::View { genesis, ledger, block_height } => {
                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                // Print information about the ledger
                Ok(format!("{:#?}", ledger.get_block(block_height)?))
            }

            Commands::ViewAccountBalance { genesis, ledger, address } => {
                let ledger = util::open_ledger(genesis, ledger)?;
                let addr = Address::<Network>::from_str(&address)?;

                Ok(format!("{address} balance {}", util::get_balance(addr, &ledger)?))
            }
        }
    }
}
