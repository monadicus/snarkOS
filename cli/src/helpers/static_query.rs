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
    console::{
        network::Network,
        program::StatePath,
        types::Field,
    },
    prelude::{
        query::QueryTrait,
    },
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct StaticQuery<N: Network> {
    pub state_root: N::StateRoot,
    pub block_height: u32,
}

#[derive(Deserialize)]
struct StaticQueryInput {
    state_root: String,
    height: u32,
}

impl<N: Network> TryFrom<&String> for StaticQuery<N> {
    type Error = anyhow::Error;

    fn try_from(s: &String) -> Result<Self> {
        if !s.trim().starts_with('{') {
            return Err(anyhow!("Not a static query"));
        }

        let input: StaticQueryInput = serde_json::from_str(s)
            .map_err(|e| anyhow!("Invalid JSON format in static query: {e}"))?;
        let state_root = N::StateRoot::from_str(&input.state_root)
            .map_err(|_| anyhow!("Invalid state root format"))?;

        Ok(Self {
            state_root,
            block_height: input.height,
        })
    }
}

#[async_trait(?Send)]
impl<N: Network> QueryTrait<N> for StaticQuery<N> {
    fn current_state_root(&self) -> Result<N::StateRoot> {
        Ok(self.state_root)
    }

    async fn current_state_root_async(&self) -> Result<N::StateRoot> {
        Ok(self.state_root)
    }

    fn get_state_path_for_commitment(&self, _commitment: &Field<N>) -> Result<StatePath<N>> {
        Err(anyhow!("StaticQuery does not support state path resolution"))
    }

    async fn get_state_path_for_commitment_async(&self, _commitment: &Field<N>) -> Result<StatePath<N>> {
        Err(anyhow!("StaticQuery does not support state path resolution"))
    }

    fn current_block_height(&self) -> Result<u32> {
        Ok(self.block_height)
    }

    async fn current_block_height_async(&self) -> Result<u32> {
        Ok(self.block_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snarkvm::console::network::TestnetV0;

    #[test]
    fn test_static_query_parse() {
        let json = r#"{"state_root": "sr1dz06ur5spdgzkguh4pr42mvft6u3nwsg5drh9rdja9v8jpcz3czsls9geg", "height": 14}"#.to_string();
        let res = StaticQuery::<TestnetV0>::try_from(&json);
        assert!(res.is_ok());
    }
}
