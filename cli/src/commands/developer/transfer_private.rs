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
    helpers::args::{network_id_parser, parse_private_key, prepare_endpoint},
};
use snarkvm::{
    console::network::{CanaryV0, MainnetV0, Network, TestnetV0},
    ledger::store::helpers::memory::BlockMemory,
    prelude::{
        Address,
        Locator,
        VM,
        Value,
        query::Query,
        store::{ConsensusStore, helpers::memory::ConsensusMemory},
    },
};

use aleo_std::StorageMode;
use anyhow::{Result, bail};
use clap::Parser;
use std::{path::PathBuf, str::FromStr};
use ureq::http::Uri;
use zeroize::Zeroize;

/// Executes the `transfer_private` function in the `credits.aleo` program.
#[derive(Debug, Parser)]
#[clap(
    group(clap::ArgGroup::new("mode").required(true).multiple(false))
)]
pub struct TransferPrivate {
    /// Specify the network to create an execution for.
    /// [options: 0 = mainnet, 1 = testnet, 2 = canary]
    #[clap(long, default_value_t=MainnetV0::ID, long, value_parser = network_id_parser())]
    network: u16,
    /// The input record used to craft the transfer.
    #[clap(long)]
    input_record: String,
    /// The recipient address.
    #[clap(long)]
    recipient: String,
    /// The number of microcredits to transfer.
    #[clap(long)]
    amount: u64,
    /// The private key used to generate the deployment.
    #[clap(short = 'p', long, group = "key")]
    private_key: Option<String>,
    /// Specify the path to a file containing the account private key of the node
    #[clap(long, group = "key")]
    private_key_file: Option<String>,
    /// The endpoint to query node state from and broadcast to (if set to broadcast).
    #[clap(short, long)]
    endpoint: Uri,
    /// The priority fee in microcredits.
    #[clap(long)]
    priority_fee: u64,
    /// The record to spend the fee from.
    #[clap(long)]
    fee_record: String,
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
    /// Specify the path to a directory containing the ledger. Overrides the default path.
    #[clap(long = "storage_path")]
    pub storage_path: Option<PathBuf>,
}

impl Drop for TransferPrivate {
    /// Zeroize the private key when the `TransferPrivate` struct goes out of scope.
    fn drop(&mut self) {
        self.private_key.zeroize();
    }
}

impl TransferPrivate {
    /// Creates an Aleo transfer with the provided inputs.
    #[allow(clippy::format_in_format_args)]
    pub fn execute(self) -> Result<String> {
        // Construct the transfer for the specified network.
        match self.network {
            MainnetV0::ID => self.construct_transfer_private::<MainnetV0>(),
            TestnetV0::ID => self.construct_transfer_private::<TestnetV0>(),
            CanaryV0::ID => self.construct_transfer_private::<CanaryV0>(),
            unknown_id => bail!("Unknown network ID ({unknown_id})"),
        }
    }

    /// Construct and process the `transfer_private` transaction.
    fn construct_transfer_private<N: Network>(&self) -> Result<String> {
        let endpoint = prepare_endpoint(self.endpoint.clone())?;

        // Specify the query
        let query = Query::<N, BlockMemory<N>>::from(endpoint.clone());

        // Retrieve the recipient.
        let recipient = Address::<N>::from_str(&self.recipient)?;

        // Retrieve the private key.
        let private_key = parse_private_key(self.private_key.clone(), self.private_key_file.clone())?;
        println!("📦 Creating private transfer of {} microcredits to {}...\n", self.amount, recipient);

        // Generate the transfer_private transaction.
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

            // Prepare the fee.
            let fee_record = Developer::parse_record(&private_key, &self.fee_record)?;
            let priority_fee = self.priority_fee;

            // Prepare the inputs for a transfer.
            let input_record = Developer::parse_record(&private_key, &self.input_record)?;
            let inputs = [
                Value::Record(input_record),
                Value::from_str(&format!("{recipient}"))?,
                Value::from_str(&format!("{}u64", self.amount))?,
            ];

            // Create a new transaction.
            vm.execute(
                &private_key,
                ("credits.aleo", "transfer_private"),
                inputs.iter(),
                Some(fee_record),
                priority_fee,
                Some(&query),
                rng,
            )?
        };
        let locator = Locator::<N>::from_str("credits.aleo/transfer_private")?;
        println!("✅ Created private transfer of {} microcredits to {}\n", &self.amount, recipient);

        // Determine if the transaction should be broadcast, stored, or displayed to the user.
        Developer::handle_transaction(
            &endpoint,
            self.broadcast,
            self.dry_run,
            &self.store,
            self.store_format,
            transaction,
            locator.to_string(),
        )
    }
}
