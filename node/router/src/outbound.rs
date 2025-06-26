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

use crate::{Router, messages::Message};
use snarkvm::prelude::Network;

use std::net::SocketAddr;

pub trait Outbound<N: Network> {
    /// Returns a reference to the router.
    fn router(&self) -> &Router<N>;

    /// Returns `true` if the node is synced up to the latest block (within the given tolerance).
    fn is_block_synced(&self) -> bool;

    /// Returns the greatest block height of any connected peer.
    fn greatest_peer_block_height(&self) -> Option<u32>;

    /// Returns the number of blocks this node is behind the greatest peer height.
    fn num_blocks_behind(&self) -> u32;

    /// Sends the given message to every connected peer, excluding the sender and any specified peer IPs.
    fn propagate(&self, message: Message<N>, excluded_peers: &[SocketAddr]) {
        // TODO (howardwu): Serialize large messages once only.
        // // Perform ahead-of-time, non-blocking serialization just once for applicable objects.
        // if let Message::UnconfirmedSolution(ref mut message) = message {
        //     if let Ok(serialized_solution) = Data::serialize(message.solution.clone()).await {
        //         let _ = std::mem::replace(&mut message.solution, Data::Buffer(serialized_solution));
        //     } else {
        //         error!("Solution serialization is bugged");
        //     }
        // } else if let Message::UnconfirmedTransaction(ref mut message) = message {
        //     if let Ok(serialized_transaction) = Data::serialize(message.transaction.clone()).await {
        //         let _ = std::mem::replace(&mut message.transaction, Data::Buffer(serialized_transaction));
        //     } else {
        //         error!("Transaction serialization is bugged");
        //     }
        // }

        // Prepare the peers to send to.
        let connected_peers = self.router().connected_peers();
        let peers = connected_peers.iter().filter(|peer_ip| !excluded_peers.contains(peer_ip));

        // Iterate through all peers that are not the sender and excluded peers.
        for peer_ip in peers {
            self.router().send(*peer_ip, message.clone());
        }
    }

    /// Sends the given message to every connected validator, excluding the sender and any specified IPs.
    fn propagate_to_validators(&self, message: Message<N>, excluded_peers: &[SocketAddr]) {
        // TODO (howardwu): Serialize large messages once only.
        // // Perform ahead-of-time, non-blocking serialization just once for applicable objects.
        // if let Message::UnconfirmedSolution(ref mut message) = message {
        //     if let Ok(serialized_solution) = Data::serialize(message.solution.clone()).await {
        //         let _ = std::mem::replace(&mut message.solution, Data::Buffer(serialized_solution));
        //     } else {
        //         error!("Solution serialization is bugged");
        //     }
        // } else if let Message::UnconfirmedTransaction(ref mut message) = message {
        //     if let Ok(serialized_transaction) = Data::serialize(message.transaction.clone()).await {
        //         let _ = std::mem::replace(&mut message.transaction, Data::Buffer(serialized_transaction));
        //     } else {
        //         error!("Transaction serialization is bugged");
        //     }
        // }

        // Prepare the peers to send to.
        let connected_validators = self.router().connected_validators();
        let peers = connected_validators.iter().filter(|peer_ip| !excluded_peers.contains(peer_ip));

        // Iterate through all validators that are not the sender and excluded validators.
        for peer_ip in peers {
            self.router().send(*peer_ip, message.clone());
        }
    }
}
