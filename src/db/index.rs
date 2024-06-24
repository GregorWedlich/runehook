use std::str::FromStr;

use bitcoin::absolute::LockTime;
use bitcoin::transaction::TxOut;
use bitcoin::Network;
use bitcoin::OutPoint;
use bitcoin::ScriptBuf;
use bitcoin::Sequence;
use bitcoin::Transaction;
use bitcoin::TxIn;
use bitcoin::Txid;
use bitcoin::Witness;
use chainhook_sdk::types::BitcoinTransactionData;
use chainhook_sdk::{types::BitcoinBlockData, utils::Context};
use ordinals::Artifact;
use ordinals::Runestone;
use tokio_postgres::Client;

use super::cache::index_cache::IndexCache;

pub fn get_rune_genesis_block_height(network: Network) -> u64 {
    match network {
        Network::Bitcoin => 840_000,
        Network::Testnet => todo!(),
        Network::Signet => todo!(),
        Network::Regtest => todo!(),
        _ => todo!(),
    }
}

fn bitcoin_tx_from_chainhook_tx(
    block: &BitcoinBlockData,
    tx: &BitcoinTransactionData,
) -> Transaction {
    Transaction {
        version: 2,
        lock_time: LockTime::from_time(block.timestamp).unwrap(),
        input: tx
            .metadata
            .inputs
            .iter()
            .map(|input| TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_str(&input.previous_output.txid.hash[2..]).unwrap(),
                    vout: input.previous_output.vout,
                },
                script_sig: ScriptBuf::from_bytes(hex::decode(&input.script_sig[2..]).unwrap()),
                sequence: Sequence(input.sequence),
                witness: Witness::new(), // We don't need this for runes
            })
            .collect(),
        output: tx
            .metadata
            .outputs
            .iter()
            .map(|output| TxOut {
                value: output.value,
                script_pubkey: ScriptBuf::from_bytes(output.get_script_pubkey_bytes()),
            })
            .collect(),
    }
}

pub async fn index_block(
    pg_client: &mut Client,
    index_cache: &mut IndexCache,
    block: &mut BitcoinBlockData,
    ctx: &Context,
) {
    let stopwatch = std::time::Instant::now();
    let block_height = block.block_identifier.index;
    info!(ctx.expect_logger(), "Indexing block {}...", block_height);
    let mut db_tx = pg_client
        .transaction()
        .await
        .expect("Unable to begin block processing pg transaction");
    for tx in block.transactions.iter() {
        let transaction = bitcoin_tx_from_chainhook_tx(block, tx);
        let tx_index = tx.metadata.index;
        let tx_id = &tx.transaction_identifier.hash;
        index_cache
            .begin_transaction(block_height, tx_index, tx_id, block.timestamp)
            .await;
        if let Some(artifact) = Runestone::decipher(&transaction) {
            match artifact {
                Artifact::Runestone(runestone) => {
                    index_cache
                        .apply_runestone(
                            &runestone,
                            &tx.metadata.inputs,
                            &tx.metadata.outputs,
                            &mut db_tx,
                            ctx,
                        )
                        .await;
                    if let Some(etching) = runestone.etching {
                        index_cache.apply_etching(&etching, &mut db_tx, ctx).await;
                    }
                    if let Some(mint_rune_id) = runestone.mint {
                        index_cache.apply_mint(&mint_rune_id, &mut db_tx, ctx).await;
                    }
                    for edict in runestone.edicts.iter() {
                        index_cache.apply_edict(edict, &mut db_tx, ctx).await;
                    }
                }
                Artifact::Cenotaph(cenotaph) => {
                    index_cache
                        .apply_cenotaph(&cenotaph, &tx.metadata.inputs, &mut db_tx, ctx)
                        .await;
                    if let Some(etching) = cenotaph.etching {
                        index_cache
                            .apply_cenotaph_etching(&etching, &mut db_tx, ctx)
                            .await;
                    }
                    if let Some(mint_rune_id) = cenotaph.mint {
                        index_cache
                            .apply_cenotaph_mint(&mint_rune_id, &mut db_tx, ctx)
                            .await;
                    }
                }
            }
        }
        index_cache.end_transaction(&mut db_tx, ctx);
    }
    index_cache.db_cache.flush(&mut db_tx, ctx).await;
    db_tx
        .commit()
        .await
        .expect("Unable to commit pg transaction");
    info!(
        ctx.expect_logger(),
        "Block {} indexed in {}s",
        block_height,
        stopwatch.elapsed().as_millis() / 1000
    );
}
