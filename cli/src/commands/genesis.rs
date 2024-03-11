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

use anyhow::{ensure, Result};
use clap::Parser;
use colored::Colorize;
use indexmap::IndexMap;
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use snarkvm::{
    console::{account::PrivateKey, network::MainnetV0, program::Network, types::Address},
    ledger::committee::{Committee, MIN_VALIDATOR_STAKE},
    utilities::ToBytes,
};

use crate::commands::{load_or_compute_genesis, DEVELOPMENT_MODE_RNG_SEED};

#[derive(Debug, Parser)]
pub struct Genesis {
    /// The private key to use when generating the genesis block. Generates one randomly if not passed.
    #[clap(name = "genesis-key", short, long)]
    genesis_key: Option<PrivateKey<MainnetV0>>,
    /// Where to write the genesis block to.
    #[clap(name = "output", short, long, default_value = "genesis.block")]
    output: PathBuf,
    /// The committee size. Not used if --bonded-balances is set.
    #[clap(name = "committee-size", long, default_value_t = 4)]
    committee_size: u16,
    /// Additional number of accounts that aren't validators to add balances to.
    #[clap(name = "additional-accounts", long, default_value_t = 0)]
    additional_accounts: u16,
    /// The balance to add to the number of accounts specified by additional-accounts.
    #[clap(name = "additional-accounts-balance", long, default_value_t = 100_000_000)]
    additional_accounts_balance: u64,
    /// A place to write out the additionally generated accounts by --additional-accounts.
    #[clap(name = "additional-accounts-file", long)]
    additional_accounts_file: Option<PathBuf>,
    /// The seed to use when generating committee private keys and the genesis block. If unpassed, uses DEVELOPMENT_MODE_RNG_SEED.
    #[clap(name = "seed", long)]
    seed: Option<u64>,
    /// The bonded balance each bonded address receives.
    #[clap(name = "bonded-balance", long, default_value_t = 1_000_000_000_000)]
    bonded_balance: u64,
    /// A place to optionally write out the generated committee private keys JSON.
    #[clap(name = "committee-file", long)]
    committee_file: Option<PathBuf>,
}

impl Genesis {
    pub fn parse(self) -> Result<String> {
        ensure!(
            self.bonded_balance >= MIN_VALIDATOR_STAKE,
            "Validator stake is too low: {} < {MIN_VALIDATOR_STAKE}",
            self.bonded_balance
        );

        let mut rng = ChaChaRng::seed_from_u64(self.seed.unwrap_or(DEVELOPMENT_MODE_RNG_SEED));

        // Generate a genesis key if one was not passed.
        let genesis_key = match self.genesis_key {
            Some(genesis_key) => genesis_key,
            None => PrivateKey::<MainnetV0>::new(&mut rng)?,
        };

        let genesis_addr = Address::try_from(&genesis_key)?;

        // The addresses and private keys of members of the committee.
        let mut committee_members = IndexMap::new();

        // Bonded balances of the committee.
        let mut bonded_balances = IndexMap::new();

        // Members of the committee.
        let mut members = IndexMap::new();

        // Public balances, including those in the committee.
        let mut public_balances = IndexMap::new();

        for i in 0..self.committee_size {
            let (key, addr) = match i {
                0 => (genesis_key, genesis_addr),
                _ => {
                    let key = PrivateKey::<MainnetV0>::new(&mut rng)?;
                    let addr = Address::try_from(&key)?;
                    (key, addr)
                }
            };

            committee_members.insert(addr, (key, self.bonded_balance));
            bonded_balances.insert(addr, (addr, self.bonded_balance));
            members.insert(addr, (self.bonded_balance, true));
            public_balances.insert(addr, self.bonded_balance);
        }

        // Construct the committee.
        let committee = Committee::<MainnetV0>::new(0u64, members)?;

        // Add additional accounts to the public balances
        let accounts = (0..self.additional_accounts)
            .map(|_| {
                let key = PrivateKey::<MainnetV0>::new(&mut rng)?;
                let addr = Address::try_from(&key)?;
                public_balances.insert(addr, self.additional_accounts_balance);
                Ok((addr, (key, self.additional_accounts_balance)))
            })
            .collect::<Result<IndexMap<_, _>>>()?;

        // Calculate the public balance per validator.
        let remaining_balance = MainnetV0::STARTING_SUPPLY
            .saturating_sub(committee.total_stake())
            .saturating_sub(public_balances.values().sum());

        if remaining_balance > 0 {
            let (_, (_, balance)) = committee_members.get_index_mut(0).unwrap();
            *balance += remaining_balance;
            let (_, balance) = public_balances.get_index_mut(0).unwrap();
            *balance += remaining_balance;
        }

        // Check if the sum of committee stakes and public balances equals the total starting supply.
        let public_balances_sum: u64 = public_balances.values().sum();
        if committee.total_stake() + public_balances_sum != MainnetV0::STARTING_SUPPLY {
            println!(
                "Sum of committee stakes and public balances does not equal total starting supply:
                                {} + {public_balances_sum} != {}",
                committee.total_stake(),
                MainnetV0::STARTING_SUPPLY
            );
        }

        // Construct the genesis block.
        let block = load_or_compute_genesis(genesis_key, committee, public_balances, bonded_balances, &mut rng)?;

        println!();

        // Write the genesis block.
        block.write_le(fs::File::options().append(false).create(true).write(true).open(&self.output)?)?;

        // Print the genesis block private key if we generated one.
        if self.genesis_key.is_none() {
            println!("The genesis block private key is: {}", genesis_key.to_string().cyan());
        }

        // Print some info about the new genesis block.
        println!("Genesis block written to {}.", self.output.display().to_string().yellow());

        // Write the accounts JSON file.
        if let Some(accounts_file) = self.additional_accounts_file {
            let file = fs::File::options().append(false).create(true).write(true).open(&accounts_file)?;
            serde_json::to_writer_pretty(file, &accounts)?;

            println!("Additional accounts written to {}.", accounts_file.display().to_string().yellow());
        } else if self.additional_accounts > 0 {
            println!("Additional accounts (each given {} balance):", self.additional_accounts_balance);
            for (addr, (key, _)) in accounts {
                println!("\t{}: {}", addr.to_string().yellow(), key.to_string().cyan());
            }
        }

        // Write the committee JSON file.
        if let Some(committee_file) = self.committee_file {
            let file = fs::File::options().append(false).create(true).write(true).open(&committee_file)?;
            serde_json::to_writer_pretty(file, &committee_members)?;

            println!("Generated committee written to {}.", committee_file.display().to_string().yellow());
        } else {
            println!("Generated committee:");
            for (addr, (key, _)) in committee_members {
                println!("\t{}: {}", addr.to_string().yellow(), key.to_string().cyan());
            }
        }

        println!();

        Ok(format!("Genesis block hash: {}", block.hash().to_string().yellow()))
    }
}
