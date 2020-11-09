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

#[macro_use]
extern crate tracing;

use snarkos::{
    cli::CLI,
    config::{Config, ConfigCli},
    display::render_init,
    miner::MinerInstance,
};
use snarkos_consensus::{ConsensusParameters, MemoryPool, MerkleTreeLedger};
use snarkos_dpc::base_dpc::{instantiated::Components, parameters::PublicParameters, BaseDPCComponents};
use snarkos_errors::node::NodeError;
use snarkos_models::algorithms::{CRH, SNARK};
use snarkos_network::{external::protocol::SyncHandler, internal::context::Context, Server};
use snarkos_objects::{AccountAddress, Network};
use snarkos_posw::PoswMarlin;
use snarkos_rpc::start_rpc_server;
use snarkos_utilities::{to_bytes, ToBytes};

use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tokio::{runtime::Runtime, sync::Mutex};
use tracing_futures::Instrument;
use tracing_subscriber::EnvFilter;

/// Builds a node from configuration parameters.
/// 1. Creates new storage database or uses existing.
/// 2. Creates new memory pool or uses existing from storage.
/// 3. Creates consensus parameters.
/// 4. Creates network server.
/// 5. Starts rpc server thread.
/// 6. Starts miner thread.
/// 7. Starts network server listener.
async fn start_server(config: Config) -> Result<(), NodeError> {
    // Load the node's IP and port.
    let address = format! {"{}:{}", config.node.ip, config.node.port};
    // Parse the socket's address.
    let socket_address = address.parse::<SocketAddr>()?;

    // Load the path for the database from the config.
    let mut path = config.node.dir;
    path.push(&config.node.db);
    // Create storage database using the path.
    let storage = Arc::new(MerkleTreeLedger::open_at_path(path.clone())?);

    // Create the memory pool using the storage.
    let memory_pool = MemoryPool::from_storage(&storage.clone())?;
    // Create a new mutex on the memory pool.
    let memory_pool_lock = Arc::new(Mutex::new(memory_pool.clone()));

    // Fetch the first bootnode address from config.
    let bootnode = match config.p2p.bootnodes.len() {
        0 => socket_address,
        _ => config.p2p.bootnodes[0].parse::<SocketAddr>()?,
    };

    // A SyncHandler manages syncing chain state with a sync node.
    let sync_handler = SyncHandler::new(bootnode);
    let sync_handler_lock = Arc::new(Mutex::new(sync_handler));

    //
    info!("Loading Aleo parameters...");
    let parameters = PublicParameters::<Components>::load(!config.miner.is_miner)?;
    info!("Loading complete.");

    // Fetch the valid inner snark ids
    let inner_snark_vk: <<Components as BaseDPCComponents>::InnerSNARK as SNARK>::VerificationParameters =
        parameters.inner_snark_parameters.1.clone().into();
    let inner_snark_id = parameters
        .system_parameters
        .inner_snark_verification_key_crh
        .hash(&to_bytes![inner_snark_vk]?)?;

    let authorized_inner_snark_ids = vec![to_bytes![inner_snark_id]?];

    // Set the initial consensus parameters.
    let consensus = ConsensusParameters {
        max_block_size: 1_000_000_000usize,
        max_nonce: u32::max_value(),
        target_block_time: 10i64,
        network: Network::from_network_id(config.aleo.network_id),
        verifier: PoswMarlin::verify_only().expect("could not instantiate PoSW verifier"),
        authorized_inner_snark_ids,
    };

    let mut context = Arc::new(Context::new(
        socket_address,
        config.p2p.mempool_interval,
        config.p2p.min_peers,
        config.p2p.max_peers,
        config.node.is_bootnode,
        config.p2p.bootnodes.clone(),
        false,
    ));

    // Start the miner task, if the mining configuration is enabled.
    if config.miner.is_miner {
        match AccountAddress::<Components>::from_str(&config.miner.miner_address) {
            Ok(miner_address) => {
                if let Some(mutable_context) = Arc::get_mut(&mut context) {
                    mutable_context.is_miner = true;
                }

                MinerInstance::new(
                    miner_address,
                    consensus.clone(),
                    parameters.clone(),
                    storage.clone(),
                    memory_pool_lock.clone(),
                    context.clone(),
                )
                .spawn();
            }
            Err(_) => info!(
                "Miner not started. Please specify a valid miner address in your ~/.snarkOS/config.toml file or by using the --miner-address option in the CLI."
            ),
        }
    }

    // Construct the server instance. Note this does not start the server.
    let server = Server::new(
        context,
        consensus.clone(),
        storage.clone(),
        parameters,
        memory_pool_lock.clone(),
        sync_handler_lock.clone(),
        15000, // 15 seconds
    );

    // Start RPC thread, if the RPC configuration is enabled.
    if config.rpc.json_rpc {
        info!("Loading Aleo parameters for RPC...");
        let proving_parameters = PublicParameters::<Components>::load(!config.miner.is_miner)?;
        info!("Loading complete.");

        // Open a secondary storage instance to prevent resource sharing and bottle-necking.
        let secondary_storage = Arc::new(MerkleTreeLedger::open_secondary_at_path(path.clone())?);

        start_rpc_server(
            config.rpc.port,
            secondary_storage.clone(),
            path,
            proving_parameters,
            server.context.clone(),
            consensus.clone(),
            memory_pool_lock.clone(),
            sync_handler_lock.clone(),
            config.rpc.username,
            config.rpc.password,
        )
        .await?;
    }

    // Start the main server thread.
    server.listen().instrument(debug_span!("server")).await?;

    Ok(())
}

fn main() -> Result<(), NodeError> {
    let arguments = ConfigCli::new();

    let config: Config = ConfigCli::parse(&arguments)?;

    match config.node.verbose {
        0 => {}
        verbosity => {
            match verbosity {
                1 => std::env::set_var("RUST_LOG", "info"),
                2 => std::env::set_var("RUST_LOG", "debug"),
                _ => std::env::set_var("RUST_LOG", "info"),
            };

            // disable undesirable logs
            let filter = EnvFilter::from_default_env().add_directive("tokio_reactor=off".parse().unwrap());

            // initialize tracing
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(false)
                .init();

            println!("{}", render_init(&config));
        }
    }

    // create a tracing span dedicated to the entire node
    let node_span = debug_span!("node");

    Runtime::new()?.block_on(start_server(config).instrument(node_span))?;

    Ok(())
}
