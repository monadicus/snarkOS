mod util;

#[derive(Debug, Deserialize, Clone)]
pub struct TxOperation {
    from: PrivateKey<MainnetV0>,
    to: Address<MainnetV0>,
    amount: u32,
}

impl FromStr for TxOperation {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Subcommand)]
pub enum LedgerCommands {
    Init {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long)]
        genesis: PathBuf,
        /// A destination path for the ledger directory.
        #[arg(required = true, short, long)]
        output: PathBuf,
    },
    Tx {
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        #[arg(required = true, long)]
        operations: Vec<TxOperation>,
        #[arg(required = true, short, long)]
        output: PathBuf,
    },
    Add {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// A destination path for the ledger directory.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        /// The seed to use when generating blocks. Defaults to the dev mode seed.
        #[arg(name = "seed", long)]
        seed: Option<u64>,
        /// The private key to use when generating the block.
        #[arg(name = "private-key", long)]
        private_key: PrivateKey<MainnetV0>,
        /// The number of transactions to add per block.
        #[arg(name = "txns-per-block", long)]
        txns_per_block: Option<usize>,
        /// The transactions file to read from. Should have been generated with `snarkos ledger tx`.
        #[arg(name = "txns-file")]
        txns_file: PathBuf,
    },
    View {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// The ledger from which to view a block.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        /// The block height to view.
        block_height: u32,
    },
    ViewAccountBalance {
        /// A path to the genesis block to initialize the ledger from.
        #[arg(required = true, short, long, default_value = "./genesis.block")]
        genesis: PathBuf,
        /// The ledger from which to view a block.
        #[arg(required = true, short, long)]
        ledger: PathBuf,
        /// The address's balance to view.
        address: String,
    },
}

impl Commands {
    pub fn parse(self) -> Result<String> {
        match self {
            Commands::Init { genesis, output } => {
                let ledger = util::open_ledger(genesis, output)?;

                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;
                println!("Genesis block hash: {}", genesis_block.hash());

                // Load the ledger and assume that it was not loaded before
                // If the ledger existed, this would fail if the genesis block differed
                Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(output))?;
                Ok(String::from("Ledger written"))
            }
            Commands::Tx { genesis, ledger, operations, output } => {
                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                let txns = operations
                    .into_iter()
                    .map(|op| make_transaction_proof::<_, _, AleoV0>(ledger.vm(), op.to, op.amount, op.from, None))
                    .collect::<Result<Vec<_>>>()?;

                let file = fs::File::options().append(false).create(true).write(true).open(&output)?;
                serde_json::to_writer_pretty(file, &txns)?;

                Ok(format!("Wrote {} transactions to {}.", txns.len(), output.display()))
            }
            Commands::Add { genesis, ledger, seed, private_key, txns_per_block, txns_file } => {
                type Vm = VM<MainnetV0, ConsensusDB<MainnetV0>>;

                let mut rng = ChaChaRng::seed_from_u64(seed.unwrap_or(DEVELOPMENT_MODE_RNG_SEED));

                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                // Ensure we aren't trying to stick too many transactions into a block
                let per_block = txns_per_block.unwrap_or(Vm::MAXIMUM_CONFIRMED_TRANSACTIONS);
                ensure!(
                    per_block <= Vm::MAXIMUM_CONFIRMED_TRANSACTIONS,
                    "too many transactions per block (max is {})",
                    Vm::MAXIMUM_CONFIRMED_TRANSACTIONS
                );

                // Load the transactions
                let txns: Vec<Transaction<MainnetV0>> = serde_json::from_reader(fs::File::open(txns_file)?)?;

                // Add the appropriate number of blocks
                let mut block_count = 0;
                for chunk in txns.chunks(per_block) {
                    let target_block = ledger.prepare_advance_to_next_beacon_block(
                        &private_key,
                        vec![],
                        vec![],
                        chunk.to_vec(),
                        &mut rng,
                    )?;

                    ledger.advance_to_next_block(&target_block)?;
                    block_count += 1;
                }

                Ok(format!("Inserted {block_count} blocks into the ledger."))
            }
            Commands::View { genesis, ledger, block_height } => {
                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                // Print information about the ledger
                Ok(format!("{:#?}", ledger.get_block(block_height)?))
            }
            Commands::ViewAccountBalance { genesis, ledger, address } => {
                // Read the genesis block
                let genesis_block = Block::<MainnetV0>::read_le(fs::File::open(genesis)?)?;

                // Load the ledger
                let ledger = Ledger::<_, ConsensusDB<MainnetV0>>::load(genesis_block, StorageMode::Custom(ledger))?;

                let addr = Address::from_str(&address)?;
                // Print information about the ledger
                Ok(format!("{address} balance {}", get_balance(addr, &ledger)?))
            }
        }
    }
}
