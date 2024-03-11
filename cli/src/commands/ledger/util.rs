use anyhow::Result;

pub fn open_ledger<N: Network>(genesis_path: PathBuf, ledger_path: PathBuf) -> Result<Ledger<N, ConsensusDB<N>>> {
    let genesis_block = Block::read_le(fs::File::open(genesis_path)?)?;

    Ledger::load(genesis_block, StorageMode::Custom(ledger_path))
}

pub fn make_transaction_proof<N: Network, C: ConsensusStorage<N>, A: Aleo<Network = N>>(
    vm: &VM<N, C>,
    address: Address<N>,
    amount: u32,
    private_key: PrivateKey<N>,
    private_key_fee: Option<PrivateKey<N>>,
) -> Result<Transaction<N>> {
    let rng = &mut rand::thread_rng();

    let query = Query::from(vm.block_store());

    // // convert amount to microcredits
    let amount_microcredits: u64 = (amount as u64) * 1_000_000;

    // fee key falls back to the private key
    let private_key_fee = private_key_fee.unwrap_or(private_key);

    // proof for the execution of the transfer function
    let execution = {
        // authorize the transfer execution
        let authorization = vm.authorize(
            &private_key,
            ProgramID::from_str("credits.aleo")?,
            Identifier::from_str("transfer_public")?,
            vec![
                Value::from_str(address.to_string().as_str())?,
                Value::from(Literal::U64(U64::new(amount_microcredits))),
            ]
            .into_iter(),
            rng,
        )?;

        // assemble the proof
        let (_, mut trace) = vm.process().read().execute::<A, _>(authorization, rng)?;
        trace.prepare(query.clone())?;
        trace.prove_execution::<A, _>("credits.aleo/transfer_public", rng)?
    };

    // compute fee for the execution
    let (min_fee, _) = execution_cost(&vm.process().read(), &execution)?;

    // proof for the fee, authorizing the execution
    let fee = {
        // authorize the fee execution
        let fee_authorization =
        // This can have a separate private key because the fee is checked to be VALID
        // and has the associated execution id.
            vm.authorize_fee_public(&private_key_fee, min_fee, 0, execution.to_execution_id()?, rng)?;

        // assemble the proof
        let (_, mut trace) = vm.process().read().execute::<A, _>(fee_authorization, rng)?;
        trace.prepare(query)?;
        trace.prove_fee::<A, _>(rng)?
    };

    // assemble the transaction
    Transaction::<N>::from_execution(execution, Some(fee))
}

fn add_block<N: Network, A: Aleo<Network = N>>(
    rng: &mut ChaChaRng,
    ledger: &Ledger<N, ConsensusDB<N>>,
    accounts: &mut Accounts<N>,
) -> Result<()> {
    let (acc1, acc2) = accounts.two_random_accounts(rng);
    let amt_to_send = rng.gen_range(10_000..get_balance(acc1.addr, ledger)?);
    println!("Sending {amt_to_send} from {} to {}", acc1.addr, acc2.addr);

    let tx = make_transaction_proof::<_, _, A>(ledger.vm(), acc1.addr, amt_to_send, acc2.pk, None)?;
    let target_block = ledger.prepare_advance_to_next_beacon_block(&acc1.pk, vec![], vec![], vec![tx], rng)?;

    println!("Generated block hash: {}", target_block.hash());
    println!("New block height: {}", target_block.height());
    println!("New block solutions: {:?}", target_block.solutions());

    // Insert the block into the ledger's block store
    ledger.advance_to_next_block(&target_block)?;

    Ok(())
}

fn get_balance<N: Network>(addr: Address<N>, ledger: &Ledger<N, ConsensusDB<N>>) -> Result<u32> {
    let balance = ledger.vm().finalize_store().get_value_confirmed(
        ProgramID::try_from("credits.aleo")?,
        Identifier::try_from("account")?,
        &Plaintext::from(Literal::Address(addr)),
    )?;

    match balance {
        Some(Value::Plaintext(Plaintext::Literal(Literal::U64(balance), _))) => Ok((*balance / 1_000_000).try_into()?),
        None => bail!("No balance found for address: {addr}"),
        _ => unreachable!(),
    }
}
