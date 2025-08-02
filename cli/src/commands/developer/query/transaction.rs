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

use crate::helpers::ureq::http_get;

use snarkvm::prelude::Network;

use anyhow::{Result, anyhow};
use clap::Parser;
use ureq::http::Uri;

#[derive(Debug, Parser)]
pub struct QueryTransaction {
    /// ID of the transaction to fetch.
    transaction_id: String,
    #[clap(long)]
    unconfirmed: bool,
}

impl QueryTransaction {
    pub fn parse<N: Network>(self, endpoint: Uri) -> Result<String> {
        // Request the latest block height from the endpoint.
        let endpoint = if self.unconfirmed {
            format!("{endpoint}{}/transaction/unconfirmed/{}", N::SHORT_NAME, self.transaction_id)
        } else {
            format!("{endpoint}{}/transaction/{}", N::SHORT_NAME, self.transaction_id)
        };

        match http_get(&endpoint) {
            Ok(mut body) => Ok(body.read_to_string()?),
            Err((err, message)) => {
                let err_msg = message.unwrap_or(err.to_string());
                Err(anyhow!("{err_msg}").context(format!("Failed to fetch transaction {}", self.transaction_id)))
            }
        }
    }
}
