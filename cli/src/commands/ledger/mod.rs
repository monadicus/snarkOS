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

use std::{fs, ops::Deref, path::PathBuf, str::FromStr};

use anyhow::{ensure, Result};
use clap::{Args, Subcommand};
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

impl FromStr for TxOperation {
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
        operations: Vec<TxOperation>,
        #[arg(required = true, short, long)]
        output: PathBuf,
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
        #[arg(name = "txns-per-block", long)]
        txs_per_block: Option<usize>,
        /// The transactions file to read from. Should have been generated with `snarkos ledger tx`.
        #[arg(name = "txns-file")]
        txs_file: PathBuf,
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
        let fmt_layer = tracing_subscriber::fmt::Layer::default();

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

            Commands::Tx { genesis, ledger, operations, output } => {
                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                let tx_span = span!(Level::INFO, "transaction proof");
                let txns = operations
                    .into_iter()
                    .map(|op| {
                        let _enter = tx_span.enter();
                        util::make_transaction_proof::<_, _, AleoV0>(ledger.vm(), op.to, op.amount, op.from, None)
                    })
                    .collect::<Result<Vec<_>>>()?;

                util::write_json_to(&output, &txns)?;

                Ok(format!("Wrote {} transactions to {}.", txns.len(), output.display()))
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
                    "minimum is above max block txns (max is {max_transactions})"
                );

                ensure!(
                    max_per_block <= max_transactions,
                    "maximum is above max block txns (max is {max_transactions})"
                );

                let mut total_txs = 0;
                let mut gen_txs = 0;

                for _ in 0..num_blocks {
                    let num_tx_per_block = rng.gen_range(min_per_block..=max_per_block);
                    total_txs += num_tx_per_block;

                    let tx_span = span!(Level::INFO, "tx generation");
                    let txs = (0..num_tx_per_block)
                        .into_par_iter()
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

            Commands::Add { genesis, ledger, private_key, txs_per_block, txs_file } => {
                let mut rng = ChaChaRng::from_entropy();

                let ledger = util::open_ledger::<Network, Db>(genesis, ledger)?;

                // Ensure we aren't trying to stick too many transactions into a block
                let per_block_max = VM::<Network, Db>::MAXIMUM_CONFIRMED_TRANSACTIONS;
                let per_block = txs_per_block.unwrap_or(per_block_max);
                ensure!(per_block <= per_block_max, "too many transactions per block (max is {})", per_block_max);

                // Load the transactions
                let txns: Vec<Transaction<MainnetV0>> = serde_json::from_reader(fs::File::open(txs_file)?)?;

                let pk = match private_key {
                    Some(pk) => pk,
                    None => PrivateKey::new(&mut rng)?,
                };

                // Add the appropriate number of blocks
                let tx_blocks_span = span!(Level::INFO, "tx into blocks").entered();
                let block_count = util::add_transaction_blocks(&ledger, pk, &txns, per_block, &mut rng)?;
                tx_blocks_span.exit();

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
