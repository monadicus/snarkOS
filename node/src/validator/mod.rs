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

mod router;

use crate::traits::NodeInterface;

use snarkos_account::Account;
use snarkos_node_bft::{ledger_service::CoreLedgerService, spawn_blocking};
use snarkos_node_cdn::CdnBlockSync;
use snarkos_node_consensus::Consensus;
use snarkos_node_rest::Rest;
use snarkos_node_router::{
    Heartbeat,
    Inbound,
    Outbound,
    Router,
    Routing,
    messages::{NodeType, PuzzleResponse, UnconfirmedSolution, UnconfirmedTransaction},
};
use snarkos_node_sync::{BlockSync, Ping};
use snarkos_node_tcp::{
    P2P,
    protocols::{Disconnect, Handshake, OnConnect, Reading},
};
use snarkvm::prelude::{
    Ledger,
    Network,
    block::{Block, Header},
    puzzle::Solution,
    store::ConsensusStorage,
};

use aleo_std::StorageMode;
use anyhow::Result;
use core::future::Future;
#[cfg(feature = "locktick")]
use locktick::parking_lot::Mutex;
#[cfg(not(feature = "locktick"))]
use parking_lot::Mutex;
use std::{
    net::SocketAddr,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::task::JoinHandle;

/// A validator is a full node, capable of validating blocks.
#[derive(Clone)]
pub struct Validator<N: Network, C: ConsensusStorage<N>> {
    /// The ledger of the node.
    ledger: Ledger<N, C>,
    /// The consensus module of the node.
    consensus: Consensus<N>,
    /// The router of the node.
    router: Router<N>,
    /// The REST server of the node.
    rest: Option<Rest<N, C, Self>>,
    /// The block synchronization logic (used in the Router impl).
    sync: Arc<BlockSync<N>>,
    /// The spawned handles.
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// The shutdown signal.
    shutdown: Arc<AtomicBool>,
    /// Keeps track of sending pings.
    ping: Arc<Ping<N>>,
}

impl<N: Network, C: ConsensusStorage<N>> Validator<N, C> {
    /// Initializes a new validator node.
    pub async fn new(
        node_ip: SocketAddr,
        bft_ip: Option<SocketAddr>,
        rest_ip: Option<SocketAddr>,
        rest_rps: u32,
        account: Account<N>,
        trusted_peers: &[SocketAddr],
        trusted_validators: &[SocketAddr],
        genesis: Block<N>,
        cdn: Option<String>,
        storage_mode: StorageMode,
        allow_external_peers: bool,
        dev_txs: bool,
        dev: Option<u16>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self> {
        // Initialize the signal handler.
        let signal_node = Self::handle_signals(shutdown.clone());

        // Initialize the ledger.
        let ledger = Ledger::load(genesis, storage_mode.clone())?;

        // Initialize the ledger service.
        let ledger_service = Arc::new(CoreLedgerService::new(ledger.clone(), shutdown.clone()));

        // Determine if the validator should rotate external peers.
        let rotate_external_peers = false;

        // Initialize the node router.
        let router = Router::new(
            node_ip,
            NodeType::Validator,
            account.clone(),
            ledger_service.clone(),
            trusted_peers,
            Self::MAXIMUM_NUMBER_OF_PEERS as u16,
            rotate_external_peers,
            allow_external_peers,
            dev.is_some(),
        )
        .await?;

        // Initialize the block synchronization logic.
        let sync = Arc::new(BlockSync::new(ledger_service.clone()));
        let locators = sync.get_block_locators()?;
        let ping = Arc::new(Ping::new(router.clone(), locators));

        // Initialize the consensus layer.
        let consensus = Consensus::new(
            account.clone(),
            ledger_service.clone(),
            sync.clone(),
            bft_ip,
            trusted_validators,
            storage_mode.clone(),
            ping.clone(),
            dev,
        )
        .await?;

        // Initialize the node.
        let mut node = Self {
            ledger: ledger.clone(),
            consensus: consensus.clone(),
            router,
            rest: None,
            sync,
            ping,
            handles: Default::default(),
            shutdown: shutdown.clone(),
        };

        // Perform sync with CDN (if enabled).
        let cdn_sync = cdn.map(|base_url| Arc::new(CdnBlockSync::new(base_url, ledger.clone(), shutdown)));

        // Initialize the transaction pool.
        node.initialize_transaction_pool(dev, dev_txs)?;

        // Initialize the REST server.
        if let Some(rest_ip) = rest_ip {
            node.rest = Some(
                Rest::start(
                    rest_ip,
                    rest_rps,
                    Some(consensus),
                    ledger.clone(),
                    Arc::new(node.clone()),
                    cdn_sync.clone(),
                )
                .await?,
            );
        }

        // Set up everything else after CDN sync is done.
        if let Some(cdn_sync) = cdn_sync {
            if let Err(error) = cdn_sync.wait().await {
                crate::log_clean_error(&storage_mode);
                node.shut_down().await;
                return Err(error);
            }
        }

        // Initialize the routing.
        node.initialize_routing().await;
        // Initialize the notification message loop.
        node.handles.lock().push(crate::start_notification_message_loop());
        // Pass the node to the signal handler.
        let _ = signal_node.set(node.clone());
        // Return the node.
        Ok(node)
    }

    /// Returns the ledger.
    pub fn ledger(&self) -> &Ledger<N, C> {
        &self.ledger
    }

    /// Returns the REST server.
    pub fn rest(&self) -> &Option<Rest<N, C, Self>> {
        &self.rest
    }
}

impl<N: Network, C: ConsensusStorage<N>> Validator<N, C> {
    // /// Initialize the transaction pool.
    // fn initialize_transaction_pool(&self, dev: Option<u16>) -> Result<()> {
    //     use snarkvm::{
    //         console::{
    //             account::ViewKey,
    //             program::{Identifier, Literal, Plaintext, ProgramID, Record, Value},
    //             types::U64,
    //         },
    //         ledger::block::transition::Output,
    //     };
    //     use std::str::FromStr;
    //
    //     // Initialize the locator.
    //     let locator = (ProgramID::from_str("credits.aleo")?, Identifier::from_str("split")?);
    //     // Initialize the record name.
    //     let record_name = Identifier::from_str("credits")?;
    //
    //     /// Searches the genesis block for the mint record.
    //     fn search_genesis_for_mint<N: Network>(
    //         block: Block<N>,
    //         view_key: &ViewKey<N>,
    //     ) -> Option<Record<N, Plaintext<N>>> {
    //         for transition in block.transitions().filter(|t| t.is_mint()) {
    //             if let Output::Record(_, _, Some(ciphertext)) = &transition.outputs()[0] {
    //                 if ciphertext.is_owner(view_key) {
    //                     match ciphertext.decrypt(view_key) {
    //                         Ok(record) => return Some(record),
    //                         Err(error) => {
    //                             error!("Failed to decrypt the mint output record - {error}");
    //                             return None;
    //                         }
    //                     }
    //                 }
    //             }
    //         }
    //         None
    //     }
    //
    //     /// Searches the block for the split record.
    //     fn search_block_for_split<N: Network>(
    //         block: Block<N>,
    //         view_key: &ViewKey<N>,
    //     ) -> Option<Record<N, Plaintext<N>>> {
    //         let mut found = None;
    //         // TODO (howardwu): Switch to the iterator when DoubleEndedIterator is supported.
    //         // block.transitions().rev().for_each(|t| {
    //         let splits = block.transitions().filter(|t| t.is_split()).collect::<Vec<_>>();
    //         splits.iter().rev().for_each(|t| {
    //             if found.is_some() {
    //                 return;
    //             }
    //             let Output::Record(_, _, Some(ciphertext)) = &t.outputs()[1] else {
    //                 error!("Failed to find the split output record");
    //                 return;
    //             };
    //             if ciphertext.is_owner(view_key) {
    //                 match ciphertext.decrypt(view_key) {
    //                     Ok(record) => found = Some(record),
    //                     Err(error) => {
    //                         error!("Failed to decrypt the split output record - {error}");
    //                     }
    //                 }
    //             }
    //         });
    //         found
    //     }
    //
    //     let self_ = self.clone();
    //     self.spawn(async move {
    //         // Retrieve the view key.
    //         let view_key = self_.view_key();
    //         // Initialize the record.
    //         let mut record = {
    //             let mut found = None;
    //             let mut height = self_.ledger.latest_height();
    //             while found.is_none() && height > 0 {
    //                 // Retrieve the block.
    //                 let Ok(block) = self_.ledger.get_block(height) else {
    //                     error!("Failed to get block at height {}", height);
    //                     break;
    //                 };
    //                 // Search for the latest split record.
    //                 if let Some(record) = search_block_for_split(block, view_key) {
    //                     found = Some(record);
    //                 }
    //                 // Decrement the height.
    //                 height = height.saturating_sub(1);
    //             }
    //             match found {
    //                 Some(record) => record,
    //                 None => {
    //                     // Retrieve the genesis block.
    //                     let Ok(block) = self_.ledger.get_block(0) else {
    //                         error!("Failed to get the genesis block");
    //                         return;
    //                     };
    //                     // Search the genesis block for the mint record.
    //                     if let Some(record) = search_genesis_for_mint(block, view_key) {
    //                         found = Some(record);
    //                     }
    //                     found.expect("Failed to find the split output record")
    //                 }
    //             }
    //         };
    //         info!("Starting transaction pool...");
    //         // Start the transaction loop.
    //         loop {
    //             tokio::time::sleep(Duration::from_secs(1)).await;
    //             // If the node is running in development mode, only generate if you are allowed.
    //             if let Some(dev) = dev {
    //                 if dev != 0 {
    //                     continue;
    //                 }
    //             }
    //
    //             // Prepare the inputs.
    //             let inputs = [Value::from(record.clone()), Value::from(Literal::U64(U64::new(1)))].into_iter();
    //             // Execute the transaction.
    //             let transaction = match self_.ledger.vm().execute(
    //                 self_.private_key(),
    //                 locator,
    //                 inputs,
    //                 None,
    //                 None,
    //                 &mut rand::thread_rng(),
    //             ) {
    //                 Ok(transaction) => transaction,
    //                 Err(error) => {
    //                     error!("Transaction pool encountered an execution error - {error}");
    //                     continue;
    //                 }
    //             };
    //             // Retrieve the transition.
    //             let Some(transition) = transaction.transitions().next() else {
    //                 error!("Transaction pool encountered a missing transition");
    //                 continue;
    //             };
    //             // Retrieve the second output.
    //             let Output::Record(_, _, Some(ciphertext)) = &transition.outputs()[1] else {
    //                 error!("Transaction pool encountered a missing output");
    //                 continue;
    //             };
    //             // Save the second output record.
    //             let Ok(next_record) = ciphertext.decrypt(view_key) else {
    //                 error!("Transaction pool encountered a decryption error");
    //                 continue;
    //             };
    //             // Broadcast the transaction.
    //             if self_
    //                 .unconfirmed_transaction(
    //                     self_.router.local_ip(),
    //                     UnconfirmedTransaction::from(transaction.clone()),
    //                     transaction.clone(),
    //                 )
    //                 .await
    //             {
    //                 info!("Transaction pool broadcasted the transaction");
    //                 let commitment = next_record.to_commitment(&locator.0, &record_name).unwrap();
    //                 while !self_.ledger.contains_commitment(&commitment).unwrap_or(false) {
    //                     tokio::time::sleep(Duration::from_secs(1)).await;
    //                 }
    //                 info!("Transaction accepted by the ledger");
    //             }
    //             // Save the record.
    //             record = next_record;
    //         }
    //     });
    //     Ok(())
    // }

    /// Initialize the transaction pool.
    fn initialize_transaction_pool(&self, dev: Option<u16>, dev_txs: bool) -> Result<()> {
        use snarkvm::console::{
            program::{Identifier, Literal, ProgramID, Value},
            types::U64,
        };
        use std::str::FromStr;

        // Initialize the locator.
        let locator = (ProgramID::from_str("credits.aleo")?, Identifier::from_str("transfer_public")?);

        // Determine whether to start the loop.
        match dev {
            // If the node is running in development mode, only generate if you are allowed.
            Some(id) => {
                // If the node is not the first node, or if we should not create dev traffic, do not start the loop.
                if id != 0 || !dev_txs {
                    return Ok(());
                }
            }
            // If the node is not running in development mode, do not generate dev traffic.
            _ => return Ok(()),
        }

        let self_ = self.clone();
        self.spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            info!("Starting transaction pool...");

            // Start the transaction loop.
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Prepare the inputs.
                let inputs = [Value::from(Literal::Address(self_.address())), Value::from(Literal::U64(U64::new(1)))];
                // Execute the transaction.
                let self__ = self_.clone();
                let transaction = match spawn_blocking!(self__.ledger.vm().execute(
                    self__.private_key(),
                    locator,
                    inputs.into_iter(),
                    None,
                    10_000,
                    None,
                    &mut rand::thread_rng(),
                )) {
                    Ok(transaction) => transaction,
                    Err(error) => {
                        error!("Transaction pool encountered an execution error - {error}");
                        continue;
                    }
                };
                // Broadcast the transaction.
                if self_
                    .unconfirmed_transaction(
                        self_.router.local_ip(),
                        UnconfirmedTransaction::from(transaction.clone()),
                        transaction.clone(),
                    )
                    .await
                {
                    info!("Transaction pool broadcasted the transaction");
                }
            }
        });
        Ok(())
    }

    /// Spawns a task with the given future; it should only be used for long-running tasks.
    pub fn spawn<T: Future<Output = ()> + Send + 'static>(&self, future: T) {
        self.handles.lock().push(tokio::spawn(future));
    }
}

#[async_trait]
impl<N: Network, C: ConsensusStorage<N>> NodeInterface<N> for Validator<N, C> {
    /// Shuts down the node.
    async fn shut_down(&self) {
        info!("Shutting down...");

        // Shut down the node.
        trace!("Shutting down the node...");
        self.shutdown.store(true, std::sync::atomic::Ordering::Release);

        // Abort the tasks.
        trace!("Shutting down the validator...");
        self.handles.lock().iter().for_each(|handle| handle.abort());

        // Shut down the router.
        self.router.shut_down().await;

        // Shut down consensus.
        trace!("Shutting down consensus...");
        self.consensus.shut_down().await;

        info!("Node has shut down.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snarkvm::prelude::{
        MainnetV0,
        VM,
        store::{ConsensusStore, helpers::memory::ConsensusMemory},
    };

    use anyhow::bail;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use std::str::FromStr;

    type CurrentNetwork = MainnetV0;

    /// Use `RUST_MIN_STACK=67108864 cargo test --release profiler --features timer` to run this test.
    #[ignore]
    #[tokio::test]
    async fn test_profiler() -> Result<()> {
        // Specify the node attributes.
        let node = SocketAddr::from_str("0.0.0.0:4130").unwrap();
        let rest = SocketAddr::from_str("0.0.0.0:3030").unwrap();
        let storage_mode = StorageMode::Development(0);
        let dev_txs = true;

        // Initialize an (insecure) fixed RNG.
        let mut rng = ChaChaRng::seed_from_u64(1234567890u64);
        // Initialize the account.
        let account = Account::<CurrentNetwork>::new(&mut rng).unwrap();
        // Initialize a new VM.
        let vm = VM::from(ConsensusStore::<CurrentNetwork, ConsensusMemory<CurrentNetwork>>::open(
            StorageMode::new_test(None),
        )?)?;
        // Initialize the genesis block.
        let genesis = vm.genesis_beacon(account.private_key(), &mut rng)?;

        println!("Initializing validator node...");

        let validator = Validator::<CurrentNetwork, ConsensusMemory<CurrentNetwork>>::new(
            node,
            None,
            Some(rest),
            10,
            account,
            &[],
            &[],
            genesis,
            None,
            storage_mode,
            false,
            dev_txs,
            None,
            Default::default(),
        )
        .await
        .unwrap();

        println!("Loaded validator node with {} blocks", validator.ledger.latest_height(),);

        bail!("\n\nRemember to #[ignore] this test!\n\n")
    }
}
