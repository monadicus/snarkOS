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

use aleo_std::StorageMode;
use anyhow::Result;
use clap::{Args, Subcommand};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use snarkvm::{
    circuit::{Aleo, AleoV0},
    console::{
        account::PrivateKey,
        network::MainnetV0,
        program::{Environment, Identifier, Literal, Network, ProgramID, Value},
        types::{Address, U64},
    },
    ledger::{
        query::Query,
        store::{helpers::rocksdb::ConsensusDB, ConsensusStorage},
        Block, Ledger, Transaction,
    },
    synthesizer::{process::execution_cost, VM},
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
        /// The seed to use when generating a new block. Defaults to the dev mode seed.
        #[arg(name = "seed", long)]
        seed: Option<u64>,
        /// The number of blocks to add  to use when generating a new block. Defaults to the dev mode seed.
        #[arg(short, long, default_value_t = 1)]
        num: u8,
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
    Cannon {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// The ledger from which to view a block.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
    },
}

fn add_block<N: Network>(rng: &mut ChaChaRng, ledger: &Ledger<N, ConsensusDB<N>>) -> Result<()> {
    // Read the target block into memory
    let private_key = PrivateKey::<N>::new(rng)?;
    let address = Address::try_from(private_key)?;

    // let partial_solution = PartialSolution::new(address, rng.gen(), KZGCommitment(rng.gen()));
    // let solution = ProverSolution::new(partial_solution, KZGProof { w: rng.gen(), random_v: None });

    // let targeT_block =

    // TODO:
    let target_block = ledger.prepare_advance_to_next_beacon_block(
        &private_key,
        vec![],
        vec![
            /* todo: add solutions */
        ],
        vec![
            /* todo: add transactions */
        ],
        rng,
    )?;

    // let target_block = Block::<MainnetV0>::read_le(fs::File::open(block)?)?;
    // println!("Taret hash: {}", target_block.hash());
    println!("Generated block hash: {}", target_block.hash());
    println!("New block height: {}", target_block.height());
    println!("New block solutions: {:?}", target_block.solutions());

    // ledger.check_next_block(&target_block, rng)?;

    // Insert the block into the ledger's block store
    // ledger.vm().add_next_block(&target_block)?;
    ledger.advance_to_next_block(&target_block)?;

    Ok(())
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
            Commands::Add { genesis, ledger, seed, num } => {
                let mut rng = ChaChaRng::seed_from_u64(seed.unwrap_or(DEVELOPMENT_MODE_RNG_SEED));

                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                (0..num).try_for_each(|_| add_block(&mut rng, &ledger))?;
                Ok(format!("Inserted {num} block(s) into ledger"))
            }
            Commands::View { genesis, ledger, block_height } => {
                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                // Print information about the ledger
                Ok(format!("{:#?}", ledger.get_block(block_height)?))
            }
            Commands::Cannon { genesis, ledger } => {
                Ok(String::from("TODO"))
                // Read the genesis block
                // let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;
                // // let foo = genesis_block.ratifications().iter().next().unwrap();
                // // match genesis_block.ratifications().iter().next().unwrap() {
                // //     Ratify::Genesis(_, balances, _) => {

                // //     }
                // // }

                // // Load the ledger
                // let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                // let txn = make_transaction_proof(ledger.vm(), String::from(""), 0, String::from(""), None)?;

                // // (0..VM::MAXIMUM_CONFIRMED_TRANSACTIONS).map(|_| Transaction::from_execution(execution, fee))

                // Ok(String::from(""))
            }
        }
    }
}

pub fn make_transaction_proof<N: Network, C: ConsensusStorage<N>, A: Aleo<Network = N>>(
    vm: &VM<N, C>,
    address: String,
    amount: u32,
    private_key: PrivateKey<N>,
    private_key_fee: Option<PrivateKey<N>>,
) -> Result<Transaction<N>> {
    let rng = &mut rand::thread_rng();

    let query = Query::from(vm.block_store());

    // convert amount to microcredits
    let amount_microcredits = (amount as u64) * 1_000_000;

    // fee key falls back to the private key
    let private_key_fee = private_key_fee.unwrap_or(private_key);

    // proof for the execution of the transfer function
    let execution = {
        // authorize the transfer execution
        let authorization = vm.authorize(
            &private_key,
            ProgramID::from_str("credits.aleo")?,
            Identifier::from_str("transfer_public")?,
            vec![Value::from_str(&address)?, Value::from(Literal::U64(U64::new(amount_microcredits)))].into_iter(),
            rng,
        )?;

        // assemble the proof
        let (_, mut trace) = vm.process().read().execute::<A, _>(authorization, rng)?;
        trace.prepare(query.clone())?;
        trace.prove_execution::<A, _>("credits.aleo/transfer_public", rng)?
    };

    // compute fee for the execution
    let (min_fee, _) = execution_cost(&vm.process().read(), &execution)?;

    // proof for the fee, authorizing the execution
    let fee = {
        // authorize the fee execution
        let fee_authorization =
        // This can have a separate private key because the fee is checked to be VALID
        // and has the associated execution id.
            vm.authorize_fee_public(&private_key_fee, min_fee, 0, execution.to_execution_id()?, rng)?;

        // assemble the proof
        let (_, mut trace) = vm.process().read().execute::<A, _>(fee_authorization, rng)?;
        trace.prepare(query)?;
        trace.prove_fee::<A, _>(rng)?
    };

    // assemble the transaction
    Transaction::<N>::from_execution(execution, Some(fee))
}
