use anyhow::Result;
use snarkos_node::snarkvm::{
    console::network::MainnetV0, ledger::Block, parameters::mainnet::GenesisBytes, utilities::FromBytes,
};
use std::fs;

fn main() -> Result<()> {
    if fs::metadata("genesis.json").is_err() {
        if let Ok(block) = Block::<MainnetV0>::read_le(GenesisBytes::load_bytes()) {
            fs::write("genesis.json", serde_json::to_string_pretty(&block)?)?;
            return Ok(());
        }
    }
    Ok(())
}
