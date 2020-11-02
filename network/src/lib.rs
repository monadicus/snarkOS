// Copyright (C) 2019-2020 Aleo Systems Inc.
// This file is part of the snarkOS library.

// The snarkOS library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkOS library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkOS library. If not, see <https://www.gnu.org/licenses/>.

// Compilation
#![warn(unused_extern_crates)]
#![forbid(unsafe_code)]
// Documentation
#![cfg_attr(nightly, feature(doc_cfg, external_doc))]
#![cfg_attr(nightly, doc(include = "../documentation/concepts/network_server.md"))]

#[macro_use]
extern crate log;
#[macro_use]
extern crate snarkos_metrics;

pub mod external;

pub mod blocks;
pub use blocks::*;

pub mod environment;
pub use environment::*;

pub mod errors;
pub use errors::*;

pub mod inbound;
pub use inbound::*;

pub mod outbound;
pub use outbound::*;

pub mod peers;
pub use peers::*;

use crate::{
    blocks::Blocks,
    environment::Environment,
    inbound::Response,
    peers::peers::Peers,
    Inbound,
    NetworkError,
    Outbound,
};

use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, task, time::sleep};

pub(crate) type Sender = tokio::sync::mpsc::Sender<Response>;

pub(crate) type Receiver = tokio::sync::mpsc::Receiver<Response>;

/// A core data structure for operating the networking stack of this node.
#[derive(Clone)]
pub struct Server {
    /// The parameters and settings of this node server.
    environment: Environment,
    /// The inbound handler of this node server.
    inbound: Arc<RwLock<Inbound>>,
    /// The outbound handler of this node server.
    outbound: Arc<RwLock<Outbound>>,

    peers: Peers,
    blocks: Blocks,
    // sync_manager: Arc<Mutex<SyncManager>>,
}

impl Server {
    /// Creates a new instance of `Server`.
    pub async fn new(environment: &mut Environment) -> Result<Self, NetworkError> {
        // Create the inbound and outbound service.
        let inbound = Arc::new(RwLock::new(Inbound::new()));
        let outbound = Arc::new(RwLock::new(Outbound::new()));

        let peers = Peers::new(&mut environment.clone(), outbound.clone())?;
        let blocks = Blocks::new(&mut environment.clone(), outbound.clone())?;

        environment.set_managers(outbound.clone());

        Ok(Self {
            environment: environment.clone(),
            inbound,
            outbound,
            peers,
            blocks,
        })
    }

    #[inline]
    pub async fn start(self) -> Result<(), NetworkError> {
        debug!("Initializing server");
        self.inbound.write().await.listen(&self.environment).await?;
        let peers = self.peers.clone();
        let blocks = self.blocks.clone();
        task::spawn(async move {
            loop {
                info!("Hello b?");
                peers.update().await.unwrap();
                blocks.update().await.unwrap();
                sleep(Duration::from_secs(10)).await;
            }
        });
        debug!("Initialized server");
        loop {
            self.receiver().await?;
        }
    }

    pub async fn receiver(&self) -> Result<(), NetworkError> {
        warn!("START NEXT RECEIVER INBOUND");
        let response = self
            .inbound
            .write()
            .await
            .receiver()
            .lock()
            .await
            .recv()
            .await
            .ok_or(NetworkError::ReceiverFailedToParse)?;

        match response {
            Response::Transaction(source, transaction) => {
                debug!("Received transaction from {} for memory pool", source);
                let connected_peers = self.peers.connected_peers().await;
                self.blocks
                    .received_transaction(source, transaction, connected_peers)
                    .await?;
            }
            Response::Block(remote_address, block, propagate) => {
                debug!("Receiving a block from {}", remote_address);
                let connected_peers = match propagate {
                    true => Some(self.peers.connected_peers().await),
                    false => None,
                };
                self.blocks
                    .received_block(remote_address, block, connected_peers)
                    .await?;
                debug!("Received a block from {}", remote_address);
            }
            Response::GetBlock(remote_address, getblock) => {
                debug!("Receiving a getblock from {}", remote_address);
                self.blocks.received_get_block(remote_address, getblock).await?;
                debug!("Received a getblock from {}", remote_address);
            }
            Response::GetMemoryPool(remote_address) => {
                debug!("Receiving a getmemorypool from {}", remote_address);
                self.blocks.received_get_memory_pool(remote_address).await?;
                debug!("Received a getmemorypool from {}", remote_address);
            }
            Response::MemoryPool(mempool) => {
                debug!("Receiving a memorypool");
                self.blocks.received_memory_pool(mempool).await?;
                debug!("Received a memorypool");
            }
            Response::GetSync(remote_address, getsync) => {
                debug!("Receiving a getsync from {}", remote_address);
                self.blocks.received_get_sync(remote_address, getsync).await?;
                debug!("Received a getsync from {}", remote_address);
            }
            Response::Sync(remote_address, sync) => {
                debug!("Receiving a sync from {}", remote_address);
                self.blocks.received_sync(sync).await?;
                debug!("Received a sync from {}", remote_address);
            }
            Response::VersionToVerack(remote_address, remote_version) => {
                debug!("Received `Version` request from {}", remote_version.receiver);
                self.peers.version_to_verack(remote_address, &remote_version).await?;
            }
            Response::ConnectingTo(remote_address, nonce) => {
                self.peers.connecting_to_peer(&remote_address, nonce).await?;
                debug!("Connecting to {}", remote_address);
            }
            Response::Verack(remote_address, verack) => {
                trace!("RESOLVING CONNECTED TO FROM {}", remote_address);
                self.peers.connected_to_peer(&remote_address, verack.nonce).await?;
                debug!("Connected to {}", remote_address);
            }
            Response::DisconnectFrom(remote_address) => {
                debug!("Disconnecting from {}", remote_address);
                self.peers.disconnected_from_peer(&remote_address).await?;
                debug!("Disconnected from {}", remote_address);
            }
            Response::GetPeers(remote_address) => {
                self.peers.get_peers(remote_address).await?;
            }
            Response::Peers(remote_address, peers) => {
                self.peers.inbound_peers(remote_address, peers).await?;
            }
        }
        warn!("END RECEIVER INBOUND");
        Ok(())
    }
}
