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

use std::{ops::Deref, path::PathBuf, str::FromStr};

use anyhow::{ensure, Result};
use clap::{Args, Subcommand};
use indicatif::ParallelProgressIterator;
use rand::{seq::SliceRandom, thread_rng, CryptoRng, Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::Deserialize;
use snarkvm::{
    circuit::AleoV0,
    console::{account::PrivateKey, network::MainnetV0, types::Address},
    ledger::{store::helpers::rocksdb::ConsensusDB, Transaction},
    synthesizer::VM,
};
use tracing::{span, Level};
use tracing_subscriber::layer::SubscriberExt;

mod util;

type Network = MainnetV0;
type Db = ConsensusDB<Network>;

#[derive(Debug, Args)]
pub struct Command {
    #[arg(long)]
    pub enable_profiling: bool,
    #[command(subcommand)]
    pub command: Commands,
}

impl Command {
    pub fn parse(self) -> Result<String> {
        self.command.parse(self.enable_profiling)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct TxOperation {
    from: PrivateKey<Network>,
    to: Address<Network>,
    amount: u64,
}

#[derive(Debug, Clone, Deserialize)]
/// This wrapper allows for '--operations=[{}, {}]' instead of '--operations {} --operations {}'
pub struct TxOperations(pub Vec<TxOperation>);

impl FromStr for TxOperations {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Clone)]
pub struct PrivateKeys(Vec<PrivateKey<Network>>);
impl FromStr for PrivateKeys {
    type Err = <PrivateKey<Network> as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.split(',').map(PrivateKey::<Network>::from_str).collect::<Result<Vec<_>>>()?))
    }
}

impl Deref for PrivateKeys {
    type Target = Vec<PrivateKey<Network>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PrivateKeys {
    /// Returns a random 2 or 3 private keys.
    fn random_accounts<R: Rng + CryptoRng>(&self, rng: &mut R) -> Vec<PrivateKey<Network>> {
        let num = rng.gen_range(2..=3);
        let chosen = self.0.choose_multiple(rng, num);

        chosen.copied().collect()
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
        operations: TxOperations,
    },
    AddRandom {
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        #[arg(long)]
        block_private_key: Option<PrivateKey<Network>>,
        #[arg(required = true, long)]
        private_keys: PrivateKeys,
        #[arg(short, long, default_value_t = 5)]
        num_blocks: u8,
        /// Minimum number of transactions per block.
        #[arg(long, default_value_t = 128)]
        min_per_block: usize,
        /// Maximumnumber of transactions per block.
        #[arg(long, default_value_t = 1024)]
        max_per_block: usize,
        /// Maximum transaction credit transfer. If unspecified, maximum is entire account balance.
        #[arg(long)]
        max_tx_credits: Option<u64>,
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
        private_key: Option<PrivateKey<Network>>,
        /// The number of transactions to add per block.
        #[arg(name = "txs-per-block", long)]
        txs_per_block: Option<usize>,
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
    pub fn parse(self, enable_profiling: bool) -> Result<String> {
        // Initialize logging.
        let fmt_layer = tracing_subscriber::fmt::Layer::default().with_writer(std::io::stderr);

        let (flame_layer, _guard) = if enable_profiling {
            let (flame_layer, guard) = tracing_flame::FlameLayer::with_file("./tracing.folded").unwrap();
            (Some(flame_layer), Some(guard))
        } else {
            (None, None)
        };

        let subscriber = tracing_subscriber::registry::Registry::default().with(fmt_layer).with(flame_layer);
        tracing::subscriber::set_global_default(subscriber).unwrap();

        match self {
            Commands::Init { genesis, output } => {
                let ledger = util::open_ledger::<Network, Db>(genesis, output)?;
                let genesis_block = ledger.get_block(0)?;

                Ok(format!("Ledger written, genesis block hash: {}", genesis_block.hash()))
            }

            Commands::Tx { genesis, ledger, operations } => {
                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                let num_txs = operations.0.len();
                let gen_txs = operations
                    .0
                    // rayon for free parallelism
                    .into_par_iter()
                    // progress bar
                    .progress_count(num_txs as u64)
                    // generate proofs
                    .map(|op| {
                        util::make_transaction_proof::<_, _, AleoV0>(ledger.vm(), op.to, op.amount, op.from, None)
                    })
                    // discard failed transactions
                    .filter_map(Result::ok)
                    // print each transaction to stdout
                    .inspect(|proof| println!("{}", serde_json::to_string(&proof).expect("serialize proof")))
                    // take the count of succeeeded proofs
                    .count();

                eprintln!("Wrote {} transactions.", gen_txs);
                Ok(String::from(""))
            }

            Commands::AddRandom {
                genesis,
                ledger,
                block_private_key,
                private_keys,
                num_blocks,
                min_per_block,
                max_per_block,
                max_tx_credits,
            } => {
                let mut rng = ChaChaRng::from_entropy();

                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                // TODO: do this for each block?
                let block_private_key = match block_private_key {
                    Some(key) => key,
                    None => PrivateKey::<Network>::new(&mut rng)?,
                };

                let max_transactions = VM::<Network, Db>::MAXIMUM_CONFIRMED_TRANSACTIONS;

                ensure!(
                    min_per_block <= max_transactions,
                    "minimum is above max block txs (max is {max_transactions})"
                );

                ensure!(
                    max_per_block <= max_transactions,
                    "maximum is above max block txs (max is {max_transactions})"
                );

                let mut total_txs = 0;
                let mut gen_txs = 0;

                for _ in 0..num_blocks {
                    let num_tx_per_block = rng.gen_range(min_per_block..=max_per_block);
                    total_txs += num_tx_per_block;

                    let tx_span = span!(Level::INFO, "tx generation");
                    let txs = (0..num_tx_per_block)
                        .into_par_iter()
                        .progress_count(num_tx_per_block as u64)
                        .map(|_| {
                            let _enter = tx_span.enter();

                            let mut rng = ChaChaRng::from_rng(thread_rng())?;

                            let keys = private_keys.random_accounts(&mut rng);

                            let from = Address::try_from(keys[1])?;
                            let amount = match max_tx_credits {
                                Some(amount) => rng.gen_range(1..amount),
                                None => rng.gen_range(1..util::get_balance(from, &ledger)?),
                            };

                            let to = Address::try_from(keys[0])?;

                            let proof_span = span!(Level::INFO, "tx generation proof");
                            let _enter = proof_span.enter();

                            util::make_transaction_proof::<_, _, AleoV0>(
                                ledger.vm(),
                                to,
                                amount,
                                keys[1],
                                keys.get(2).copied(),
                            )
                        })
                        .filter_map(Result::ok)
                        .collect::<Vec<_>>();

                    gen_txs += txs.len();
                    let target_block = ledger.prepare_advance_to_next_beacon_block(
                        &block_private_key,
                        vec![],
                        vec![],
                        txs,
                        &mut rng,
                    )?;

                    ledger.advance_to_next_block(&target_block)?;
                }

                Ok(format!("Generated {gen_txs} transactions ({} failed)", total_txs - gen_txs))
            }

            Commands::Add { genesis, ledger, private_key, txs_per_block } => {
                let mut rng = ChaChaRng::from_entropy();

                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                // Ensure we aren't trying to stick too many transactions into a block
                let per_block_max = VM::<Network, Db>::MAXIMUM_CONFIRMED_TRANSACTIONS;
                let per_block = txs_per_block.unwrap_or(per_block_max);
                ensure!(per_block <= per_block_max, "too many transactions per block (max is {})", per_block_max);

                // Get the block private key
                let private_key = match private_key {
                    Some(pk) => pk,
                    None => PrivateKey::new(&mut rng)?,
                };

                // Stdin line buffer
                let mut buf = String::new();

                // Transaction buffer
                let mut tx_buf: Vec<Transaction<Network>> = Vec::with_capacity(per_block);

                // Macro to commit a block into the buffer
                // This can't trivially be a closure because of... you guessed it... the borrow checker
                let mut num_blocks = 0;
                macro_rules! commit_block {
                    () => {
                        let buf_size = tx_buf.len();
                        let block = util::add_block_with_transactions(
                            &ledger,
                            private_key,
                            std::mem::replace(&mut tx_buf, Vec::with_capacity(per_block)),
                            &mut rng,
                        )?;

                        println!(
                            "Inserted a block with {buf_size} transactions to the ledger (hash: {})",
                            block.hash()
                        );
                        num_blocks += 1;
                    };
                }

                loop {
                    // Clear the buffer
                    buf.clear();

                    // Read a line, and match on how many characters we read
                    match std::io::stdin().read_line(&mut buf)? {
                        // We've reached EOF
                        0 => {
                            if !tx_buf.is_empty() {
                                commit_block!();
                            }
                            break;
                        }

                        // Not at EOF
                        _ => {
                            // Remove newline from buffer
                            buf.pop();

                            // Skip if buffer is now empty
                            if buf.is_empty() {
                                continue;
                            }

                            // Deserialize the transaction
                            let Ok(tx) = serde_json::from_str(&buf) else {
                                eprintln!("Failed to deserialize transaction: {buf}");
                                continue;
                            };

                            // Commit if the buffer is now big enough
                            tx_buf.push(tx);
                            if tx_buf.len() >= per_block {
                                commit_block!();
                            }
                        }
                    }
                }

                Ok(format!("Inserted {num_blocks} blocks into the ledger"))
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
