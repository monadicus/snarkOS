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

mod transfer_private;
pub use transfer_private::*;

use crate::helpers::{http_get_json, http_post_json};

use ureq::http::Uri;

use snarkvm::{
    console::network::Network,
    package::Package,
    prelude::{
        Address,
        CanaryV0,
        Ciphertext,
        Identifier,
        Literal,
        MainnetV0,
        Plaintext,
        PrivateKey,
        Program,
        ProgramID,
        Record,
        TestnetV0,
        ToBytes,
        Value,
        ViewKey,
        block::Transaction,
    },
};

use anyhow::{Result, anyhow, bail, ensure};
use clap::{Parser, ValueEnum};
use colored::Colorize;
use std::{path::PathBuf, str::FromStr};

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum StoreFormat {
    String,
    Bytes,
}

/// Commands to deploy and execute transactions
#[derive(Debug, Parser)]
pub enum Developer {
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
}

impl Developer {
    pub fn execute(self) -> Result<String> {
        match self {
            Self::Decrypt(decrypt) => decrypt.execute(),
            Self::Deploy(deploy) => deploy.execute(),
            Self::Execute(execute) => execute.execute(),
            Self::Scan(scan) => scan.execute(),
            Self::TransferPrivate(transfer_private) => transfer_private.execute(),
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

    /// Fetch the program from the given endpoint.
    fn fetch_program<N: Network>(program_id: &ProgramID<N>, endpoint: &Uri) -> Result<Program<N>> {
        // Get the network being used.
        let network = match N::ID {
            snarkvm::console::network::MainnetV0::ID => "mainnet",
            snarkvm::console::network::TestnetV0::ID => "testnet",
            snarkvm::console::network::CanaryV0::ID => "canary",
            unknown_id => bail!("Unknown network ID ({unknown_id})"),
        };

        // Send a request to the query node.
        http_get_json(&format!("{endpoint}/{network}/program/{program_id}")).map_err(|(err, message)| {
            let err_msg = message.unwrap_or(err.to_string());

            // Debug formatting displays more useful info, especially if the response body is empty.
            anyhow!("Failed to fetch program {program_id}: {err_msg}")
        })
    }

    /// Fetch the public balance in microcredits associated with the address from the given endpoint.
    fn get_public_balance<N: Network>(address: &Address<N>, endpoint: &Uri) -> Result<u64> {
        // Initialize the program id and account identifier.
        let credits = ProgramID::<N>::from_str("credits.aleo")?;
        let account_mapping = Identifier::<N>::from_str("account")?;

        // Get the network being used.
        let network = match N::ID {
            snarkvm::console::network::MainnetV0::ID => "mainnet",
            snarkvm::console::network::TestnetV0::ID => "testnet",
            snarkvm::console::network::CanaryV0::ID => "canary",
            unknown_id => bail!("Unknown network ID ({unknown_id})"),
        };

        // Send a request to the query node.
        let result = http_get_json::<Option<Value<N>>>(&format!(
            "{endpoint}/{network}/program/{credits}/mapping/{account_mapping}/{address}"
        ));

        // Deserialize the balance.
        let result = result.map_err(|(err, msg)| match err {
            ureq::Error::StatusCode(_status) => {
                anyhow!(msg.unwrap_or("Response too large!".to_string()))
            }
            _ => err.into(),
        });

        // Return the balance in microcredits.
        match result {
            Ok(Some(Value::Plaintext(Plaintext::Literal(Literal::<N>::U64(amount), _)))) => Ok(*amount),
            Ok(None) => Ok(0),
            Ok(Some(..)) => bail!("Failed to deserialize balance for {address}"),
            Err(err) => bail!("Failed to fetch balance for {address}: {err:?}"),
        }
    }

    /// Determine if the transaction should be broadcast or displayed to user.
    #[allow(clippy::too_many_arguments)]
    fn handle_transaction<N: Network>(
        endpoint: &Uri,
        network: u16,
        broadcast: bool,
        dry_run: bool,
        store: &Option<String>,
        store_format: StoreFormat,
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
        if broadcast {
            let network = match network {
                CanaryV0::ID => "canary",
                TestnetV0::ID => "testnet",
                MainnetV0::ID => "mainnet",
                _ => bail!("Invalid network id: {network}"),
            };

            let endpoint = format!("{endpoint}/{network}/transaction/broadcast");

            let result: Result<String, _> = http_post_json(&endpoint, &transaction);

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
                                endpoint
                            )
                        }
                        Transaction::Execute(..) => {
                            println!(
                                "⌛ Execution {transaction_id} ('{}') has been broadcast to {}.",
                                operation.bold(),
                                endpoint
                            )
                        }
                        Transaction::Fee(..) => {
                            println!("❌ Failed to broadcast fee '{}' to the {}.", operation.bold(), endpoint)
                        }
                    }
                }
                Err((error, message)) => {
                    let error_message = match error {
                        ureq::Error::StatusCode(code) => {
                            if let Some(message) = message {
                                format!("(status code {code}: {message})")
                            } else {
                                format!("(status code {code})")
                            }
                        }
                        _ => format!("({error})"),
                    };

                    match transaction {
                        Transaction::Deploy(..) => {
                            bail!("❌ Failed to deploy '{}' to {}: {}", operation.bold(), &endpoint, error_message)
                        }
                        Transaction::Execute(..) => {
                            bail!(
                                "❌ Failed to broadcast execution '{}' to {}: {}",
                                operation.bold(),
                                &endpoint,
                                error_message
                            )
                        }
                        Transaction::Fee(..) => {
                            bail!(
                                "❌ Failed to broadcast fee '{}' to {}: {}",
                                operation.bold(),
                                &endpoint,
                                error_message
                            )
                        }
                    }
                }
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
