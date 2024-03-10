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

mod account;
use std::{fs, path::PathBuf, str::FromStr};

pub use account::*;

mod clean;
pub use clean::*;

mod developer;
pub use developer::*;

mod start;
use indexmap::IndexMap;
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use serde::{Deserialize, Serialize};
use snarkvm::{
    console::{account::PrivateKey, network::MainnetV0, program::Network, types::Address},
    ledger::committee::{Committee, MIN_DELEGATOR_STAKE, MIN_VALIDATOR_STAKE},
    utilities::ToBytes,
};
pub use start::*;

mod update;
pub use update::*;

mod ledger;

use anstyle::{AnsiColor, Color, Style};
use anyhow::{bail, ensure, Result};
use clap::{builder::Styles, Parser};

const HEADER_COLOR: Option<Color> = Some(Color::Ansi(AnsiColor::Yellow));
const LITERAL_COLOR: Option<Color> = Some(Color::Ansi(AnsiColor::Green));
const STYLES: Styles = Styles::plain()
    .header(Style::new().bold().fg_color(HEADER_COLOR))
    .usage(Style::new().bold().fg_color(HEADER_COLOR))
    .literal(Style::new().bold().fg_color(LITERAL_COLOR));

#[derive(Debug, Parser)]
#[clap(name = "snarkOS", author = "The Aleo Team <hello@aleo.org>", styles = STYLES)]
pub struct CLI {
    /// Specify the verbosity [options: 0, 1, 2, 3]
    #[clap(default_value = "2", short, long)]
    pub verbosity: u8,
    /// Specify a subcommand.
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Balances(IndexMap<Address<MainnetV0>, u64>);
impl FromStr for Balances {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Parser)]
pub enum Command {
    #[clap(subcommand)]
    Account(Account),
    #[clap(name = "clean")]
    Clean(Clean),
    #[clap(subcommand)]
    Developer(Developer),
    #[clap(name = "start")]
    Start(Box<Start>),
    #[clap(name = "update")]
    Update(Update),
    #[clap(name = "genesis")]
    Genesis {
        /// The private key to use when generating the genesis block. Generates one randomly if not passed.
        #[clap(name = "genesis-key", short, long)]
        genesis_key: Option<PrivateKey<MainnetV0>>,
        /// Where to write the genesis block to.
        #[clap(name = "output", short, long, default_value = "genesis.block")]
        output: PathBuf,
        /// The committee size. Not used if --bonded-balances is set.
        #[clap(name = "committee-size", long, default_value_t = 4)]
        committee_size: u16,
        /// Additional addresses that aren't validators to add balances to.
        #[clap(name = "additional-accounts", long, default_value_t = 4)]
        additional_accounts: u16,
        /// Additional addresses that aren't validators to add balances to.
        #[clap(name = "account_balance", long, default_value_t = 100_000_000)]
        account_balance: u64,
        /// Additional addresses that aren't validators to add balances to.
        #[clap(name = "accounts-file", long, default_value = "accounts.json")]
        accounts_file: Option<PathBuf>,
        /// The seed to use when generating committee private keys and the genesis block. If unpassed, uses DEVELOPMENT_MODE_RNG_SEED.
        #[clap(name = "seed", long)]
        seed: Option<u64>,
        /// A JSON map of addresses to balances. If unset, private keys will be generated at block creation.
        #[clap(name = "bonded-balance", long, default_value_t = 1_000_000_000_000)]
        bonded_balance: u64,
        /// A place to optionally write out the generated committee private keys JSON.
        #[clap(name = "committee-file", long, default_value = "committee.json")]
        committee_file: Option<PathBuf>,
    },
    #[clap(name = "ledger")]
    Ledger(ledger::Command),
}

impl Command {
    /// Parses the command.
    pub fn parse(self) -> Result<String> {
        match self {
            Self::Account(command) => command.parse(),
            Self::Clean(command) => command.parse(),
            Self::Developer(command) => command.parse(),
            Self::Start(command) => command.parse(),
            Self::Update(command) => command.parse(),
            Self::Genesis {
                genesis_key: genesis_key_input,
                output,
                committee_size,
                additional_accounts,
                account_balance,
                accounts_file,
                seed,
                bonded_balance,
                committee_file,
            } => {
                use colored::Colorize;

                let mut rng = ChaChaRng::seed_from_u64(seed.unwrap_or(DEVELOPMENT_MODE_RNG_SEED));

                // Generate a genesis key if one was not passed.
                let genesis_key = match genesis_key_input {
                    Some(genesis_key) => genesis_key,
                    None => PrivateKey::<MainnetV0>::new(&mut rng)?,
                };
                let genesis_addr = Address::try_from(&genesis_key)?;

                let mut comittee_members = IndexMap::new();
                comittee_members.insert(genesis_addr, (genesis_key, bonded_balance));
                let mut bonded_balances = IndexMap::new();
                bonded_balances.insert(genesis_addr, (genesis_addr, bonded_balance));
                let mut members = IndexMap::new();
                let mut public_balances = IndexMap::new();

                ensure!(
                    bonded_balance >= MIN_VALIDATOR_STAKE,
                    format!("Validator stake is too low: {bonded_balance} < {MIN_VALIDATOR_STAKE}")
                );

                for _ in 0..committee_size {
                    let key = PrivateKey::<MainnetV0>::new(&mut rng)?;
                    let addr = Address::try_from(&key)?;
                    comittee_members.insert(addr, (key, bonded_balance));
                    bonded_balances.insert(addr, (addr, bonded_balance));
                    members.insert(addr, (bonded_balance, true));
                    public_balances.insert(addr, bonded_balance);
                }

                // Construct the committee.
                let committee = Committee::<MainnetV0>::new(0u64, members)?;

                // Add additional accounts to the public balances
                let accounts = (0..additional_accounts)
                    .map(|_| {
                        let key = PrivateKey::<MainnetV0>::new(&mut rng)?;
                        let addr = Address::try_from(&key)?;
                        public_balances.insert(addr, account_balance);
                        Ok((addr, (key, account_balance)))
                    })
                    .collect::<Result<IndexMap<_, _>>>()?;

                // Calculate the public balance per validator.
                let remaining_balance = MainnetV0::STARTING_SUPPLY
                    .saturating_sub(committee.total_stake())
                    .saturating_sub(public_balances.values().sum());

                if remaining_balance > 0 {
                    let (_, (_, balance)) = comittee_members.get_index_mut(0).unwrap();
                    *balance += remaining_balance;
                    let (_, balance) = public_balances.get_index_mut(0).unwrap();
                    *balance += remaining_balance;
                }

                // // Check if the sum of committee stakes and public balances equals the total starting supply.
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
                let block =
                    load_or_compute_genesis(genesis_key, committee, public_balances, bonded_balances, &mut rng)?;

                println!();

                // Write the genesis block.
                block.write_le(fs::File::options().append(false).create(true).write(true).open(&output)?)?;

                // Print the genesis block private key if we generated one.
                if genesis_key_input.is_none() {
                    println!("The genesis block private key is: {}", genesis_key.to_string().cyan());
                }

                // Print some info about the new genesis block.
                println!("Genesis block written to {}.", output.display().to_string().yellow());

                // Write the accounts JSON file.
                serde_json::to_writer_pretty(
                    fs::File::options().append(false).create(true).write(true).open(accounts_file.unwrap())?,
                    &accounts,
                )?;

                // Print some info about the new genesis block.
                println!("Additional accounts written to {}.", output.display().to_string().yellow());

                // Write the committee JSON file.
                serde_json::to_writer_pretty(
                    fs::File::options().append(false).create(true).write(true).open(committee_file.unwrap())?,
                    &comittee_members,
                )?;

                // Print some info about the new genesis block.
                println!("Additional accounts written to {}.", output.display().to_string().yellow());

                println!();

                Ok(format!("Genesis block hash: {}", block.hash().to_string().yellow()))
            }
            Self::Ledger(ledger::Command { command }) => command.parse(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // As per the official clap recommendation.
    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        CLI::command().debug_assert()
    }
}
