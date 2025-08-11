// Copyright (c) 2019-2025 Provable Inc.
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

use super::Developer;
use crate::{
    commands::StoreFormat,
    helpers::args::{parse_private_key, prepare_endpoint},
};

use snarkvm::{
    console::network::Network,
    ledger::{query::QueryTrait, store::helpers::memory::BlockMemory},
    prelude::{
        Address,
        ConsensusVersion,
        Identifier,
        Locator,
        Process,
        ProgramID,
        VM,
        Value,
        query::Query,
        store::{ConsensusStore, helpers::memory::ConsensusMemory},
    },
};

use aleo_std::StorageMode;
use anyhow::{Context, Result, bail};
use clap::Parser;
use colored::Colorize;
use std::{path::PathBuf, str::FromStr};
use tracing::debug;
use ureq::http::Uri;
use zeroize::Zeroize;

/// Executes an Aleo program function.
#[derive(Debug, Parser)]
#[command(
    group(clap::ArgGroup::new("mode").required(true).multiple(false)),
    group(clap::ArgGroup::new("key").required(true).multiple(false))
)]
pub struct Execute {
    /// The program identifier.
    program_id: String,
    /// The function name.
    function: String,
    /// The function inputs.
    inputs: Vec<String>,
    /// The private key used to generate the execution.
    #[clap(short = 'p', long, group = "key")]
    private_key: Option<String>,
    /// Specify the path to a file containing the account private key of the node
    #[clap(long, group = "key")]
    private_key_file: Option<String>,
    /// Use a developer validator key tok generate the deployment.
    #[clap(long, group = "key")]
    dev_key: Option<u16>,
    /// The endpoint to query node state from and broadcast to (if set to broadcast).
    #[clap(short, long)]
    endpoint: Uri,
    /// The priority fee in microcredits.
    #[clap(long, default_value_t = 0)]
    priority_fee: u64,
    /// The record to spend the fee from.
    #[clap(short, long)]
    record: Option<String>,
    /// The endpoint used to broadcast the generated transaction.
    #[clap(short, long, group = "mode")]
    broadcast: bool,
    /// Performs a dry-run of transaction generation.
    #[clap(short, long, group = "mode")]
    dry_run: bool,
    /// Store generated deployment transaction to a local file.
    #[clap(long, group = "mode")]
    store: Option<String>,
    /// If --store is specified, the format in which the transaction should be stored : string or
    /// bytes, by default : bytes.
    #[clap(long, value_enum, default_value_t = StoreFormat::Bytes, requires="store")]
    store_format: StoreFormat,
    /// Wait for the transaction to be accepted by the network. Requires --broadcast.
    #[clap(long, requires = "broadcast")]
    wait: bool,
    /// Timeout in seconds when waiting for transaction confirmation. Default is 60 seconds.
    #[clap(long, default_value_t = 60, requires = "wait")]
    timeout: u64,
    /// Specify the path to a directory containing the ledger. Overrides the default path.
    #[clap(long = "storage_path")]
    storage_path: Option<PathBuf>,
}

impl Drop for Execute {
    /// Zeroize the private key when the `Execute` struct goes out of scope.
    fn drop(&mut self) {
        if let Some(mut pk) = self.private_key.take() {
            pk.zeroize()
        }
    }
}

impl Execute {
    /// Executes an Aleo program function with the provided inputs.
    pub fn parse<N: Network>(self) -> Result<String> {
        let endpoint = prepare_endpoint(self.endpoint.clone())?;

        // Specify the query
        let query = Query::<N, BlockMemory<N>>::from(endpoint.clone());

        // TODO (kaimast): can this ever be true?
        let is_static_query = matches!(query, Query::STATIC(_));

        // Retrieve the private key.
        let private_key = parse_private_key(self.private_key.clone(), self.private_key_file.clone(), self.dev_key)?;

        // Retrieve the program ID.
        let program_id = ProgramID::from_str(&self.program_id).with_context(|| "Failed to parse program ID")?;

        // Retrieve the function.
        let function = Identifier::from_str(&self.function).with_context(|| "Failed to parse function ID")?;

        // Retrieve the inputs.
        let inputs = self.inputs.iter().map(|input| Value::from_str(input)).collect::<Result<Vec<Value<N>>>>()?;

        let locator = Locator::<N>::from_str(&format!("{program_id}/{function}"))?;
        println!("📦 Creating execution transaction for '{}'...\n", &locator.to_string().bold());

        // Generate the execution transaction.
        let transaction = {
            // Initialize an RNG.
            let rng = &mut rand::thread_rng();

            // Initialize the storage.
            let storage_mode = match &self.storage_path {
                Some(path) => StorageMode::Custom(path.clone()),
                None => StorageMode::Production,
            };
            let store = ConsensusStore::<N, ConsensusMemory<N>>::open(storage_mode)?;

            // Initialize the VM.
            let vm = VM::from(store)?;

            if !is_static_query {
                let height = query.current_block_height()?;
                let version = N::CONSENSUS_VERSION(height)?;
                debug!("At block height {height} and consensus {version:?}");

                // Ensure the correct edition is used if on newer consensus versions.
                let edition = if version < ConsensusVersion::V8 { 0 } else { 1 };

                // Load the program and it's imports into the process.
                load_program(&query, &mut vm.process().write(), &program_id, edition)?;
            }

            // Prepare the fee.
            let fee_record = match &self.record {
                Some(record_string) => Some(
                    Developer::parse_record(&private_key, record_string).with_context(|| "Failed to parse record")?,
                ),
                None => None,
            };

            // Create a new transaction.
            vm.execute(
                &private_key,
                (program_id, function),
                inputs.iter(),
                fee_record,
                self.priority_fee,
                Some(&query),
                rng,
            )
            .with_context(|| "VM failed to execute transaction locally")?
        };

        // Check if the public balance is sufficient.
        if self.record.is_none() && !is_static_query {
            // Fetch the public balance.
            let address = Address::try_from(&private_key)?;
            let public_balance = query
                .get_public_balance(&address)
                .with_context(|| "Failed to check for sufficient funds to send transaction")?;

            // Check if the public balance is sufficient.
            let storage_cost = transaction
                .execution()
                .with_context(|| "Failed to get execution cost of transaction")?
                .size_in_bytes()?;

            // Calculate the base fee.
            // This fee is the minimum fee required to pay for the transaction,
            // excluding any finalize fees that the execution may incur.
            let base_fee = storage_cost.saturating_add(self.priority_fee);

            // If the public balance is insufficient, return an error.
            if public_balance < base_fee {
                bail!(
                    "❌ The public balance of {} is insufficient to pay the base fee for `{}`",
                    public_balance,
                    locator.to_string().bold()
                );
            }
        }

        println!("✅ Created execution transaction for '{}'", locator.to_string().bold());

        // Determine if the transaction should be broadcast, stored, or displayed to the user.
        Developer::handle_transaction(
            &endpoint,
            self.broadcast,
            self.dry_run,
            &self.store,
            self.store_format,
            self.wait,
            self.timeout,
            transaction,
            locator.to_string(),
        )
    }
}

/// A helper function to recursively load the program and all of its imports into the process.
fn load_program<N: Network>(
    query: &Query<N, BlockMemory<N>>,
    process: &mut Process<N>,
    program_id: &ProgramID<N>,
    edition: u16,
) -> Result<()> {
    // Fetch the program.
    let program = query.get_program(program_id).with_context(|| "Failed to fetch program")?;

    // Return early if the program is already loaded.
    if process.contains_program(program.id()) {
        return Ok(());
    }

    // Iterate through the program imports.
    for import_program_id in program.imports().keys() {
        // Add the imports to the process if does not exist yet.
        if !process.contains_program(import_program_id) {
            // Recursively load the program and its imports.
            load_program(query, process, import_program_id, edition)
                .with_context(|| format!("Failed to load imported program {import_program_id}"))?;
        }
    }

    // Add the program to the process if it does not already exist.
    if !process.contains_program(program.id()) {
        process
            .add_programs_with_editions(&[(program, edition)])
            .with_context(|| format!("Failed to add program {program_id}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CLI, Command, DeveloperCommand};

    #[test]
    fn clap_snarkos_execute() -> Result<()> {
        let arg_vec = &[
            "snarkos",
            "developer",
            "execute",
            "--private-key",
            "PRIVATE_KEY",
            "--endpoint=ENDPOINT",
            "--priority-fee",
            "77",
            "--record",
            "RECORD",
            "--dry-run",
            "hello.aleo",
            "hello",
            "1u32",
            "2u32",
        ];
        let cli = CLI::try_parse_from(arg_vec)?;

        let Command::Developer(developer) = cli.command else {
            bail!("Unexpected result of clap parsing!");
        };
        let DeveloperCommand::Execute(execute) = developer.command else {
            bail!("Unexpected result of clap parsing!");
        };

        assert_eq!(developer.network, 0);
        assert_eq!(execute.private_key, Some("PRIVATE_KEY".to_string()));
        assert_eq!(execute.endpoint, "ENDPOINT");
        assert_eq!(execute.priority_fee, 77);
        assert_eq!(execute.record, Some("RECORD".into()));
        assert_eq!(execute.program_id, "hello.aleo".to_string());
        assert_eq!(execute.function, "hello".to_string());
        assert_eq!(execute.inputs, vec!["1u32".to_string(), "2u32".to_string()]);

        Ok(())
    }

    #[test]
    fn clap_snarkos_execute_pk_file() -> Result<()> {
        let arg_vec = &[
            "snarkos",
            "developer",
            "execute",
            "--private-key-file",
            "PRIVATE_KEY_FILE",
            "--endpoint=ENDPOINT",
            "--record",
            "RECORD",
            "--dry-run",
            "hello.aleo",
            "hello",
            "1u32",
            "2u32",
        ];
        let cli = CLI::try_parse_from(arg_vec)?;

        let Command::Developer(developer) = cli.command else {
            bail!("Unexpected result of clap parsing!");
        };
        let DeveloperCommand::Execute(execute) = developer.command else {
            bail!("Unexpected result of clap parsing!");
        };

        assert_eq!(developer.network, 0);
        assert_eq!(execute.private_key_file, Some("PRIVATE_KEY_FILE".to_string()));
        assert_eq!(execute.endpoint, "ENDPOINT");
        assert_eq!(execute.priority_fee, 0); // Default value.
        assert_eq!(execute.record, Some("RECORD".into()));
        assert_eq!(execute.program_id, "hello.aleo".to_string());
        assert_eq!(execute.function, "hello".to_string());
        assert_eq!(execute.inputs, vec!["1u32".to_string(), "2u32".to_string()]);

        Ok(())
    }

    #[test]
    fn clap_snarkos_execute_two_private_keys() {
        let arg_vec = &[
            "snarkos",
            "developer",
            "execute",
            "--private-key",
            "PRIVATE_KEY",
            "--private-key-file",
            "PRIVATE_KEY_FILE",
            "--endpoint=ENDPOINT",
            "--priority-fee",
            "77",
            "--record",
            "RECORD",
            "--dry-run",
            "hello.aleo",
            "hello",
            "1u32",
            "2u32",
        ];

        let err = CLI::try_parse_from(arg_vec).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_snarkos_execute_no_private_keys() {
        let arg_vec = &[
            "snarkos",
            "developer",
            "execute",
            "--endpoint=ENDPOINT",
            "--priority-fee",
            "77",
            "--record",
            "RECORD",
            "--dry-run",
            "hello.aleo",
            "hello",
            "1u32",
            "2u32",
        ];

        let err = CLI::try_parse_from(arg_vec).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }
}
