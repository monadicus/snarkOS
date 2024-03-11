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

use std::{fs, path::PathBuf, str::FromStr};

use aleo_std::StorageMode;
use anyhow::{bail, ensure, Ok, Result};
use clap::{Args, Subcommand};
use indexmap::IndexMap;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use serde::Deserialize;
use snarkvm::{
    circuit::{Aleo, AleoV0},
    console::{
        account::PrivateKey,
        network::MainnetV0,
        program::{Identifier, Literal, Network, ProgramID, Value},
        types::{Address, U64},
    },
    ledger::{
        query::Query,
        store::{helpers::rocksdb::ConsensusDB, ConsensusStorage},
        Block, Ledger, Transaction,
    },
    prelude::Plaintext,
    synthesizer::{process::execution_cost, VM},
    utilities::FromBytes,
};

use crate::commands::DEVELOPMENT_MODE_RNG_SEED;

struct Accounts<N: Network>(pub IndexMap<Address<N>, (PrivateKey<N>, u64)>);
struct Account<N: Network> {
    addr: Address<N>,
    pk: PrivateKey<N>,
}

impl<N: Network> Accounts<N> {
    fn from_file(path: PathBuf) -> Result<Self> {
        let accounts = serde_json::from_reader(fs::File::open(path)?)?;
        Ok(Self(accounts))
    }

    fn two_random_accounts<'a>(&self, rng: &mut ChaChaRng) -> (Account<N>, Account<N>) {
        let len = self.0.len();
        let first_index = rng.gen_range(0..len);
        let second_index = rng.gen_range(0..len);

        let (addr1, (pk1, _)) = self.0.get_index(first_index).unwrap();
        // TODO could also be a random new account
        let (addr2, (pk2, _)) = self.0.get_index(second_index).unwrap();

        (Account { addr: *addr1, pk: *pk1 }, Account { addr: *addr2, pk: *pk2 })
    }
}
