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
        #[clap(name = "genesis-key", short, long)]
        genesis_key: PrivateKey<MainnetV0>,
        // bonded_balances: Balances,
        #[clap(name = "filename", short, long, default_value = "genesis.block")]
        filename: PathBuf,
        #[clap(name = "committee-size", long, default_value_t = 4)]
        committee_size: u16,
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
            Self::Genesis { genesis_key, filename, committee_size, .. /* bonded_balances */ } => {
                let mut rng = ChaChaRng::seed_from_u64(DEVELOPMENT_MODE_RNG_SEED);

                // Initialize the development private keys.
                let development_private_keys = (0..committee_size)
                    .map(|_| PrivateKey::<MainnetV0>::new(&mut rng))
                    .collect::<Result<Vec<_>>>()?;
                // Initialize the development addresses.
                let development_addresses =
                    development_private_keys.iter().map(Address::<MainnetV0>::try_from).collect::<Result<Vec<_>>>()?;

                // Construct the committee based on the state of the bonded balances.
                let bonded_balances = development_addresses
                    .into_iter()
                    .map(|addr| {
                        let staker_addr = addr;
                        let validator_addr = addr;
                        (staker_addr, (validator_addr, 100000000000000))
                    })
                    .collect::<IndexMap<_, _>>();

                // Construct the committee members.
                let mut members = IndexMap::new();
                for (staker_address, (validator_address, amount)) in bonded_balances.iter() {
                    // Ensure that the staking amount is sufficient.
                    match staker_address == validator_address {
                        true => ensure!(
                            amount >= &MIN_VALIDATOR_STAKE,
                            format!("Validator stake is too low: {amount} < {MIN_VALIDATOR_STAKE}")
                        ),
                        false => ensure!(
                            amount >= &MIN_DELEGATOR_STAKE,
                            format!("Delegator stake is too low: {amount} < {MIN_DELEGATOR_STAKE}")
                        ),
                    }

                    // Add or update the validator entry in the list of members.
                    members
                        .entry(*validator_address)
                        .and_modify(|(stake, _)| *stake += amount)
                        .or_insert((*amount, true));
                }
                // Construct the committee.
                let committee = Committee::<MainnetV0>::new(0u64, members)?;

                let num_committee_members = committee.members().len();

                // Calculate the public balance per validator.
                let remaining_balance = MainnetV0::STARTING_SUPPLY.saturating_sub(committee.total_stake());
                let public_balance_per_validator = remaining_balance.saturating_div(num_committee_members as u64);

                // Construct the public balances with fairly equal distribution.
                let mut public_balances = bonded_balances
                    .keys()
                    .map(|addr| (*addr, public_balance_per_validator))
                    .collect::<IndexMap<_, _>>();

                // If there is some leftover balance, add it to the 0-th validator.
                let leftover =
                    remaining_balance.saturating_sub(public_balance_per_validator * num_committee_members as u64);
                if leftover > 0 {
                    let (_, balance) = public_balances.get_index_mut(0).unwrap();
                    *balance += leftover;
                }

                // Check if the sum of committee stakes and public balances equals the total starting supply.
                let public_balances_sum: u64 = public_balances.values().sum();
                if committee.total_stake() + public_balances_sum != MainnetV0::STARTING_SUPPLY {
                    bail!(
                        "Sum of committee stakes and public balances does not equal total starting supply:
                        {} + {public_balances_sum} != {}",
                        committee.total_stake(),
                        MainnetV0::STARTING_SUPPLY
                    );
                }

                // Construct the genesis block.
                let block =
                    load_or_compute_genesis(genesis_key, committee, public_balances, bonded_balances, &mut rng)?;

                // Write the genesis block
                block.write_le(fs::File::options().append(false).create(true).write(true).open(filename)?)?;

                // print the genesis block
                Ok(format!("Genesis block: {:?}", block))
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
