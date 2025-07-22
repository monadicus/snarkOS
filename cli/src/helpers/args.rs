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

use snarkvm::console::network::{CanaryV0, MainnetV0, Network};

use clap::builder::RangedU64ValueParser;

pub(crate) fn network_id_parser() -> RangedU64ValueParser<u16> {
    RangedU64ValueParser::<u16>::new().range((MainnetV0::ID as u64)..=(CanaryV0::ID as u64))
}
