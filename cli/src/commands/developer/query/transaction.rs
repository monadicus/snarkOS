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

use snarkvm::{
    ledger::{query::Query, store::helpers::memory::BlockMemory},
    prelude::Network,
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;

use std::str::FromStr;

#[derive(Debug, Parser)]
pub struct QueryTransaction {
    /// ID of the transaction to fetch.
    transaction_id: String,
    #[clap(long)]
    unconfirmed: bool,
}

impl QueryTransaction {
    pub fn parse<N: Network>(self, query: Query<N, BlockMemory<N>>) -> Result<String> {
        let transaction_id = N::TransactionID::from_str(self.transaction_id.as_str())
            .map_err(|_| anyhow!("Failed to parse transaction ID"))?;

        let txn = if self.unconfirmed {
            query.get_unconfirmed_transaction(&transaction_id)?
        } else {
            query.get_transaction(&transaction_id)?
        };

        serde_json::to_string_pretty(&txn).with_context(|| "Failed to convert transaction to JSON string")
    }
}
