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

use std::{collections::HashMap, net::SocketAddr};

#[derive(Debug, Default)]
pub struct Resolver {
    /// The map of the listener address to (ambiguous) peer address.
    from_listener: HashMap<SocketAddr, SocketAddr>,
    /// The map of the (ambiguous) peer address to listener address.
    to_listener: HashMap<SocketAddr, SocketAddr>,
}

impl Resolver {
    /// Returns the listener address for the given (ambiguous) peer address, if it exists.
    pub fn get_listener(&self, peer_addr: &SocketAddr) -> Option<SocketAddr> {
        self.to_listener.get(peer_addr).copied()
    }

    /// Returns the (ambiguous) peer address for the given listener address, if it exists.
    pub fn get_ambiguous(&self, peer_ip: &SocketAddr) -> Option<SocketAddr> {
        self.from_listener.get(peer_ip).copied()
    }

    /// Inserts a bidirectional mapping of the listener address and the (ambiguous) peer address.
    pub fn insert_peer(&mut self, listener_ip: SocketAddr, peer_addr: SocketAddr) {
        self.from_listener.insert(listener_ip, peer_addr);
        self.to_listener.insert(peer_addr, listener_ip);
    }

    /// Removes the bidirectional mapping of the listener address and the (ambiguous) peer address.
    pub fn remove_peer(&mut self, listener_ip: &SocketAddr) {
        if let Some(peer_addr) = self.from_listener.remove(listener_ip) {
            self.to_listener.remove(&peer_addr);
        }
    }
}
