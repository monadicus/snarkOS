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

mod decrypt;
pub use decrypt::*;

mod deploy;
pub use deploy::*;

mod execute;
pub use execute::*;

mod scan;
pub use scan::*;

mod query;
pub use query::*;

mod transfer_private;
pub use transfer_private::*;

use crate::helpers::{args::network_id_parser, logger::initialize_terminal_logger};

use snarkvm::{package::Package, prelude::*};

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::{Parser, ValueEnum};
use colored::Colorize;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::PathBuf,
    str::FromStr,
    thread,
    time::{Duration, Instant},
};
use ureq::http::Uri;

/// The format to store a generated transaction as.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum StoreFormat {
    String,
    Bytes,
}

/// Commands to deploy and execute transactions
#[derive(Debug, Parser)]
pub enum DeveloperCommand {
    /// Decrypt a ciphertext.
    Decrypt(Decrypt),
    /// Deploy a program.
    Deploy(Deploy),
    /// Execute a program function.
    Execute(Execute),
    /// Scan the node for records.
    Scan(Scan),
    /// Execute the `credits.aleo/transfer_private` function.
    TransferPrivate(TransferPrivate),
    /// Get information about something on the Aleo chain.
    Query(Query),
}

const DEFAULT_ENDPOINT: &str = "https://api.explorer.provable.com/v1";

#[derive(Debug, Parser)]
pub struct Developer {
    /// The specific developer command to run.
    #[clap(subcommand)]
    command: DeveloperCommand,
    /// Specify the network to create an execution for.
    /// [options: 0 = mainnet, 1 = testnet, 2 = canary]
    #[clap(long, default_value_t=MainnetV0::ID, long, global=true, value_parser = network_id_parser())]
    network: u16,
    /// Sets verbosity of log output. By default, no logs are shown.
    #[clap(long, global = true)]
    verbosity: Option<u8>,
}

impl Developer {
    /// Runs the developer subcommand chosen by the user.
    pub fn parse(self) -> Result<String> {
        if let Some(verbosity) = self.verbosity {
            initialize_terminal_logger(verbosity).with_context(|| "Failed to initialize terminal logger")?
        }

        match self.network {
            MainnetV0::ID => self.parse_inner::<MainnetV0>(),
            TestnetV0::ID => self.parse_inner::<TestnetV0>(),
            CanaryV0::ID => self.parse_inner::<CanaryV0>(),
            unknown_id => bail!("Unknown network ID ({unknown_id})"),
        }
    }

    /// Internal logic of [`Self::parse`] for each of the different networks.
    fn parse_inner<N: Network>(self) -> Result<String> {
        use DeveloperCommand::*;
        match self.command {
            Decrypt(decrypt) => decrypt.parse::<N>(),
            Deploy(deploy) => deploy.parse::<N>(),
            Execute(execute) => execute.parse::<N>(),
            Scan(scan) => scan.parse::<N>(),
            TransferPrivate(transfer_private) => transfer_private.parse::<N>(),
            Query(query) => query.parse::<N>(),
        }
    }

    /// Parse the package from the directory.
    fn parse_package<N: Network>(program_id: ProgramID<N>, path: &Option<String>) -> Result<Package<N>> {
        // Instantiate a path to the directory containing the manifest file.
        let directory = match path {
            Some(path) => PathBuf::from_str(path)?,
            None => std::env::current_dir()?,
        };

        // Load the package.
        let package = Package::open(&directory)?;

        ensure!(
            package.program_id() == &program_id,
            "The program name in the package does not match the specified program name"
        );

        // Return the package.
        Ok(package)
    }

    /// Parses the record string. If the string is a ciphertext, then attempt to decrypt it.
    fn parse_record<N: Network>(private_key: &PrivateKey<N>, record: &str) -> Result<Record<N, Plaintext<N>>> {
        match record.starts_with("record1") {
            true => {
                // Parse the ciphertext.
                let ciphertext = Record::<N, Ciphertext<N>>::from_str(record)?;
                // Derive the view key.
                let view_key = ViewKey::try_from(private_key)?;
                // Decrypt the ciphertext.
                ciphertext.decrypt(&view_key)
            }
            false => Record::<N, Plaintext<N>>::from_str(record),
        }
    }

    /// Helper function to send a POST request with a JSON body to an endpoint and await a JSON response.
    fn http_post_json<I: Serialize, O: DeserializeOwned>(path: &str, arg: &I) -> Result<O> {
        ureq::post(path)
            .config()
            .build()
            .send_json(arg)
            .with_context(|| "HTTP POST request failed")?
            .into_body()
            .read_json()
            .with_context(|| "Failed to parse JSON response")
    }

    /// Helper function to send a GET request to an endpoint and await a JSON response.
    fn http_get_json<O: DeserializeOwned>(path: &str) -> Result<O> {
        ureq::get(path)
            .call()
            .with_context(|| "HTTP GET request failed")?
            .into_body()
            .read_json()
            .with_context(|| "Failed to parse JSON response")
    }

    /// Wait for a transaction to be confirmed by the network.
    fn wait_for_transaction_confirmation<N: Network>(
        endpoint: &Uri,
        transaction_id: &N::TransactionID,
        timeout_seconds: u64,
    ) -> Result<()> {
        let start_time = Instant::now();
        let timeout_duration = Duration::from_secs(timeout_seconds);
        let poll_interval = Duration::from_secs(2); // Poll every 2 seconds

        while start_time.elapsed() < timeout_duration {
            // Check if transaction exists in a confirmed block
            let tx_endpoint = format!("{endpoint}{}/transaction/{transaction_id}", N::SHORT_NAME);

            match Self::http_get_json::<serde_json::Value>(&tx_endpoint) {
                Ok(_) => {
                    // Transaction was found, meaning it's confirmed
                    return Ok(());
                }
                Err(_) => {
                    // Transaction not found yet, continue polling
                    thread::sleep(poll_interval);
                }
            }
        }

        // Timeout reached
        bail!("❌ Transaction {} was not confirmed within {} seconds", transaction_id, timeout_seconds);
    }

    /// Gets the public account balance of an Aleo Address (in microcredits).
    fn get_public_balance<N: Network>(endpoint: &Uri, address: &Address<N>) -> Result<u64> {
        // Initialize the program id and account identifier.
        let account_mapping = Identifier::<N>::from_str("account")?;
        let credits = ProgramID::<N>::from_str("credits.aleo")?;

        // Send a request to the query node.
        let result: Option<Value<N>> = Self::http_get_json(&format!(
            "{endpoint}{}/program/{credits}/mapping/{account_mapping}/{address}",
            N::SHORT_NAME,
        ))?;

        // Return the balance in microcredits.
        match result {
            Some(Value::Plaintext(Plaintext::Literal(Literal::<N>::U64(amount), _))) => Ok(*amount),
            Some(..) => bail!("Failed to deserialize balance for {address}"),
            None => Ok(0),
        }
    }

    /// Returns the unconfirmed transaction for the given transaction ID.
    fn get_unconfirmed_transaction<N: Network>(
        endpoint: &Uri,
        transaction_id: &N::TransactionID,
    ) -> Result<Transaction<N>> {
        Self::http_get_json(&format!("{endpoint}{}/transaction/unconfirmed/{transaction_id}", N::SHORT_NAME))
    }

    /// Determine if the transaction should be broadcast or displayed to user.
    ///
    /// This function expects that exactly one of `dry_run`, `store`, and `broadcast` are `true` (or `Some`).
    /// `broadcast` can be set to `Some(None)` to broadcast using the default endpoint.
    /// Alternatively, it can be set to `Some(Some(url))` to providifferent
    /// endpoint than that used for querying.
    #[allow(clippy::too_many_arguments)]
    fn handle_transaction<N: Network>(
        endpoint: &Uri,
        broadcast: &Option<Option<Uri>>,
        dry_run: bool,
        store: &Option<String>,
        store_format: StoreFormat,
        wait: bool,
        timeout: u64,
        transaction: Transaction<N>,
        operation: String,
    ) -> Result<String> {
        // Get the transaction id.
        let transaction_id = transaction.id();

        // Ensure the transaction is not a fee transaction.
        ensure!(!transaction.is_fee(), "The transaction is a fee transaction and cannot be broadcast");

        // Determine if the transaction should be stored.
        if let Some(path) = store {
            match PathBuf::from_str(path) {
                Ok(file_path) => {
                    match store_format {
                        StoreFormat::Bytes => {
                            let transaction_bytes = transaction.to_bytes_le()?;
                            std::fs::write(&file_path, transaction_bytes)?;
                        }
                        StoreFormat::String => {
                            let transaction_string = transaction.to_string();
                            std::fs::write(&file_path, transaction_string)?;
                        }
                    }

                    println!(
                        "Transaction {transaction_id} was stored to {} as {:?}",
                        file_path.display(),
                        store_format
                    );
                }
                Err(err) => {
                    println!("The transaction was unable to be stored due to: {err}");
                }
            }
        };

        // Determine if the transaction should be broadcast to the network.
        if let Some(broadcast_value) = broadcast {
            let broadcast_endpoint = if let Some(url) = broadcast_value {
                url.to_string()
            } else {
                format!("{endpoint}{}/transaction/broadcast", N::SHORT_NAME)
            };

            let result: Result<String, _> = Self::http_post_json(&broadcast_endpoint, &transaction);

            match result {
                Ok(response_string) => {
                    ensure!(
                        response_string == transaction_id.to_string(),
                        "The response does not match the transaction id. ({response_string} != {transaction_id})"
                    );

                    match transaction {
                        Transaction::Deploy(..) => {
                            println!(
                                "⌛ Deployment {transaction_id} ('{}') has been broadcast to {}.",
                                operation.bold(),
                                broadcast_endpoint
                            )
                        }
                        Transaction::Execute(..) => {
                            println!(
                                "⌛ Execution {transaction_id} ('{}') has been broadcast to {}.",
                                operation.bold(),
                                broadcast_endpoint
                            )
                        }
                        Transaction::Fee(..) => {
                            println!("❌ Failed to broadcast fee '{}' to the {}.", operation.bold(), broadcast_endpoint)
                        }
                    }

                    // If wait is enabled, wait for transaction confirmation
                    if wait {
                        println!("⏳ Waiting for transaction confirmation (timeout: {timeout}s)...");
                        Self::wait_for_transaction_confirmation::<N>(endpoint, &transaction_id, timeout)?;

                        match transaction {
                            Transaction::Deploy(..) => {
                                println!("✅ Deployment {} ('{}') confirmed!", transaction_id, operation.bold())
                            }
                            Transaction::Execute(..) => {
                                println!("✅ Execution {} ('{}') confirmed!", transaction_id, operation.bold())
                            }
                            Transaction::Fee(..) => {
                                println!("✅ Fee {} ('{}') confirmed!", transaction_id, operation.bold())
                            }
                        }
                    }
                }
                Err(error) => match transaction {
                    Transaction::Deploy(..) => {
                        return Err(error.context(anyhow!(
                            "Failed to deploy '{op}' to {broadcast_endpoint}",
                            op = operation.bold()
                        )));
                    }
                    Transaction::Execute(..) => {
                        return Err(error.context(anyhow!(
                            "Failed to broadcast execution '{op}' to {broadcast_endpoint}",
                            op = operation.bold()
                        )));
                    }
                    Transaction::Fee(..) => {
                        return Err(error.context(anyhow!(
                            "Failed to broadcast fee '{op}' to {broadcast_endpoint}",
                            op = operation.bold()
                        )));
                    }
                },
            };

            // Output the transaction id.
            Ok(transaction_id.to_string())
        } else if dry_run {
            // Output the transaction string.
            Ok(transaction.to_string())
        } else {
            Ok("".to_string())
        }
    }
}
