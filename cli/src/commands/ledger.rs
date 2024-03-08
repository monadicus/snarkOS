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

use std::{fs, path::PathBuf};

use aleo_std::StorageMode;
use anyhow::Result;
use clap::{Args, Subcommand};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use snarkvm::{
    algorithms::polycommit::kzg10::{KZGCommitment, KZGProof},
    console::{account::PrivateKey, network::MainnetV0, types::Address},
    ledger::{
        coinbase::{PartialSolution, ProverSolution},
        store::helpers::rocksdb::ConsensusDB,
        Block, Ledger,
    },
    utilities::FromBytes,
};

use crate::commands::DEVELOPMENT_MODE_RNG_SEED;

#[derive(Debug, Args)]
pub struct Command {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long)]
        genesis: PathBuf,
        /// A destination path for the ledger directory.
        #[arg(required = true, short)]
        output: PathBuf,
    },
    Add {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// A destination path for the ledger directory.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        // /// The block to add to the specified ledger.
        // #[arg(required = true)]
        // block: PathBuf,
    },
}

impl Commands {
    pub fn parse(self) -> Result<String> {
        match self {
            Commands::Init { genesis, output } => {
                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;
                println!("Genesis block hash: {}", genesis_block.hash());

                // Load the ledger and assume that it was not loaded before
                // If the ledger existed, this would fail if the genesis block differed
                Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(output))?;
                Ok(String::from("Ledger written"))
            }
            Commands::Add { genesis, ledger } => {
                let mut rng = ChaChaRng::seed_from_u64(DEVELOPMENT_MODE_RNG_SEED);

                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                // Read the target block into memory
                let private_key = PrivateKey::<MainnetV0>::new(&mut rng)?;
                let address = Address::try_from(private_key)?;

                let partial_solution = PartialSolution::new(address, rng.gen(), KZGCommitment(rng.gen()));
                let solution = ProverSolution::new(partial_solution, KZGProof { w: rng.gen(), random_v: None });
                let target_block = ledger.prepare_advance_to_next_beacon_block(
                    &private_key,
                    vec![],
                    vec![solution],
                    vec![],
                    &mut rng,
                )?;

                // let target_block = Block::<MainnetV0>::read_le(fs::File::open(block)?)?;
                // println!("Taret hash: {}", target_block.hash());
                println!("Generated block hash: {}", target_block.hash());
                println!("New block height: {}", target_block.height());
                println!("New block solutions: {:?}", target_block.solutions());

                // Insert the block into the ledger's block store
                ledger.vm().add_next_block(&target_block)?;
                Ok(String::from("Inserted block into ledger"))
            }
        }
    }
}
