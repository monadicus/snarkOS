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
    console::network::{CanaryV0, MainnetV0},
    prelude::{Network, PrivateKey},
};

use anyhow::{Context, Result, anyhow};
use clap::builder::RangedU64ValueParser;
use std::{path::PathBuf, str::FromStr};

pub(crate) fn network_id_parser() -> RangedU64ValueParser<u16> {
    RangedU64ValueParser::<u16>::new().range((MainnetV0::ID as u64)..=(CanaryV0::ID as u64))
}

/// Fetch the private key, either from the commandline or from a file.
pub(crate) fn parse_private_key<N: Network>(
    cmdline: Option<String>,
    file_name: Option<String>,
) -> Result<PrivateKey<N>> {
    let key_str = if let Some(keystr) = cmdline {
        keystr
    } else if let Some(file_name) = file_name {
        let path = file_name.parse::<PathBuf>().map_err(|e| anyhow!("Invalid path - {e}"))?;
        std::fs::read_to_string(path).with_context(|| "Failed to read private key from disk")?.trim().to_string()
    } else {
        unreachable!();
    };

    // Retrieve the private key.
    let private_key = PrivateKey::from_str(&key_str).with_context(|| "Failed to parse private key")?;

    Ok(private_key)
}
