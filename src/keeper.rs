use {
    crate::{helpers::to_sdk_instruction, states::PerpMarketCache},
    iter_tools::Itertools,
    solana_client::tpu_client::TpuClient,
    solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool},
    solana_sdk::{
        hash::Hash, instruction::Instruction, message::Message, signature::Keypair, signer::Signer,
        transaction::Transaction,
    },
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{Builder, JoinHandle},
    },
};

fn create_root_bank_update_instructions(perp_markets: &[PerpMarketCache]) -> Vec<Instruction> {
    perp_markets
        .iter()
        .map(|perp_market| {
            let ix = mango::instruction::update_root_bank(
                &perp_market.mango_program_pk,
                &perp_market.mango_group_pk,
                &perp_market.mango_cache_pk,
                &perp_market.root_bank,
                perp_market.node_banks.as_slice(),
            )
            .unwrap();
            to_sdk_instruction(ix)
        })
        .collect()
}

fn create_update_fundings_instructions(perp_markets: &[PerpMarketCache]) -> Vec<Instruction> {
    perp_markets
        .iter()
        .map(|perp_market| {
            let ix = mango::instruction::update_funding(
                &perp_market.mango_program_pk,
                &perp_market.mango_group_pk,
                &perp_market.mango_cache_pk,
                &perp_market.perp_market_pk,
                &perp_market.bids,
                &perp_market.asks,
            )
            .unwrap();
            to_sdk_instruction(ix)
        })
        .collect()
}

fn create_cache_root_bank_instruction(perp_markets: &[PerpMarketCache]) -> Instruction {
    let mango_program_pk = perp_markets[0].mango_program_pk;
    let mango_group_pk = perp_markets[0].mango_group_pk;
    let mango_cache_pk = perp_markets[0].mango_cache_pk;
    let root_banks = perp_markets.iter().map(|x| x.root_bank).collect_vec();

    let ix = mango::instruction::cache_root_banks(
        &mango_program_pk,
        &mango_group_pk,
        &mango_cache_pk,
        root_banks.as_slice(),
    )
    .unwrap();
    to_sdk_instruction(ix)
}

fn create_update_price_cache_instructions(perp_markets: &[PerpMarketCache]) -> Instruction {
    let mango_program_pk = perp_markets[0].mango_program_pk;
    let mango_group_pk = perp_markets[0].mango_group_pk;
    let mango_cache_pk = perp_markets[0].mango_cache_pk;
    let price_oracles = perp_markets.iter().map(|x| x.price_oracle).collect_vec();

    let ix = mango::instruction::cache_prices(
        &mango_program_pk,
        &mango_group_pk,
        &mango_cache_pk,
        price_oracles.as_slice(),
    )
    .unwrap();
    to_sdk_instruction(ix)
}

fn create_cache_perp_markets_instructions(perp_markets: &[PerpMarketCache]) -> Instruction {
    let mango_program_pk = perp_markets[0].mango_program_pk;
    let mango_group_pk = perp_markets[0].mango_group_pk;
    let mango_cache_pk = perp_markets[0].mango_cache_pk;
    let perp_market_pks = perp_markets.iter().map(|x| x.perp_market_pk).collect_vec();
    let ix = mango::instruction::cache_perp_markets(
        &mango_program_pk,
        &mango_group_pk,
        &mango_cache_pk,
        perp_market_pks.as_slice(),
    )
    .unwrap();
    to_sdk_instruction(ix)
}

pub fn send_transaction(
    tpu_client: Arc<TpuClient<QuicPool, QuicConnectionManager, QuicConfig>>,
    ixs: &[Instruction],
    blockhash: Arc<RwLock<Hash>>,
    payer: &Keypair,
) {
    let mut tx = Transaction::new_unsigned(Message::new(ixs, Some(&payer.pubkey())));
    let recent_blockhash = blockhash.read().unwrap();
    tx.sign(&[payer], *recent_blockhash);
    tpu_client.send_transaction(&tx);
}

pub fn start_keepers(
    exit_signal: Arc<AtomicBool>,
    tpu_client: Arc<TpuClient<QuicPool, QuicConnectionManager, QuicConfig>>,
    perp_markets: Vec<PerpMarketCache>,
    blockhash: Arc<RwLock<Hash>>,
    authority: &Keypair,
) -> JoinHandle<()> {
    let authority = Keypair::from_bytes(&authority.to_bytes()).unwrap();
    Builder::new()
        .name("updating root bank keeper".to_string())
        .spawn(move || {
            let root_update_ixs = create_root_bank_update_instructions(&perp_markets);
            let cache_prices = create_update_price_cache_instructions(&perp_markets);
            let update_perp_cache = create_cache_perp_markets_instructions(&perp_markets);
            let cache_root_bank_ix = create_cache_root_bank_instruction(&perp_markets);
            let update_funding_ix = create_update_fundings_instructions(&perp_markets);

            let blockhash = blockhash.clone();

            // add prioritization instruction
            //let prioritization_ix = ComputeBudgetInstruction::set_compute_unit_price(10000);
            //root_update_ixs.insert(0, prioritization_ix.clone());

            while !exit_signal.load(Ordering::Relaxed) {
                send_transaction(
                    tpu_client.clone(),
                    &[cache_prices.clone()],
                    blockhash.clone(),
                    &authority,
                );

                send_transaction(
                    tpu_client.clone(),
                    &[cache_root_bank_ix.clone()],
                    blockhash.clone(),
                    &authority,
                );

                send_transaction(
                    tpu_client.clone(),
                    update_funding_ix.as_slice(),
                    blockhash.clone(),
                    &authority,
                );

                send_transaction(
                    tpu_client.clone(),
                    root_update_ixs.as_slice(),
                    blockhash.clone(),
                    &authority,
                );

                send_transaction(
                    tpu_client.clone(),
                    &[update_perp_cache.clone()],
                    blockhash.clone(),
                    &authority,
                );

                send_transaction(
                    tpu_client.clone(),
                    &[update_perp_cache.clone()],
                    blockhash.clone(),
                    &authority,
                );

                send_transaction(
                    tpu_client.clone(),
                    &[update_perp_cache.clone()],
                    blockhash.clone(),
                    &authority,
                );
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        })
        .unwrap()
}
