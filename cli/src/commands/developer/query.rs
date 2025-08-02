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

mod transaction;
pub use transaction::QueryTransaction;

use crate::helpers::args::prepare_endpoint;

use snarkvm::prelude::Network;

use anyhow::Result;
use clap::Parser;
use ureq::http::Uri;

#[derive(Debug, Parser)]
pub enum QueryCommand {
    Transaction(QueryTransaction),
}

#[derive(Debug, Parser)]
pub struct Query {
    /// The endpoint to scan blocks from.
    #[clap(long, default_value = "https://api.explorer.provable.com/v1", global = true)]
    endpoint: Uri,

    #[clap(subcommand)]
    command: QueryCommand,
}

impl Query {
    pub fn parse<N: Network>(self) -> Result<String> {
        use QueryCommand::*;

        let endpoint = prepare_endpoint(self.endpoint)?;

        match self.command {
            Transaction(txn) => txn.parse::<N>(endpoint),
        }
    }
}
