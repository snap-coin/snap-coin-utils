// averages.rs
use anyhow::{Result, anyhow};
use bincode::encode_to_vec;
use snap_coin::{
    api::client::Client, blockchain_data_provider::BlockchainDataProvider, core::{transaction::Transaction},
};
use std::collections::HashMap;

use crate::normalize_difficulty;

#[derive(Debug)]
pub struct BlockAverages {
    pub average: f64,
    pub std_dev: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub _sample_size: usize,
}

#[derive(Debug)]
pub struct ChainStats {
    pub block_time: BlockAverages,
    pub avg_txs_per_block: f64,
    pub avg_io_per_block: f64,
    pub avg_block_size_bytes: f64,
    pub tps: f64,

    pub avg_block_difficulty: f64,
    pub avg_tx_difficulty: f64,

    pub top_miners: Vec<([u8; 32], usize)>,
    pub top_addresses: Vec<([u8; 32], usize)>,

    pub block_difficulty_series: Vec<f64>,
    pub tx_difficulty_series: Vec<f64>,
}

/// Return top N items from a frequency map
fn top_n(map: HashMap<[u8; 32], usize>, n: usize) -> Vec<([u8; 32], usize)> {
    let mut v: Vec<_> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(n);
    v
}

pub fn plot_difficulties(blocks: &[usize], block_diff: &[f64], tx_diff: &[f64]) {
    let blocks_chars = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
    let term_width = match term_size::dimensions() {
        Some((w, _)) => w,
        None => 80,
    };

    let bar_max_width = (term_width - 7 - 3 - 3) / 2; // 6 for block #, 3 for separators, divide remaining
    let max_block = block_diff.iter().cloned().fold(0.0, f64::max);
    let max_tx = tx_diff.iter().cloned().fold(0.0, f64::max);

    println!(
        "{:>6} | {:<width$} | {:<width$}",
        "Block #",
        "Block Diff",
        "TX Diff",
        width = bar_max_width
    );
    println!(
        "{:-<7}-+-{:-<width$}-+-{:-<width$}",
        "",
        "",
        "",
        width = bar_max_width
    );

    for i in 0..blocks.len() {
        // Scale individually, cap to max width
        let scale_block =
            ((block_diff[i] / max_block) * bar_max_width as f64).min(bar_max_width as f64);
        let scale_tx = ((tx_diff[i] / max_tx) * bar_max_width as f64).min(bar_max_width as f64);

        let full_block = scale_block.floor() as usize;
        let partial_block = ((scale_block - full_block as f64) * 8.0).round() as usize;
        let full_tx = scale_tx.floor() as usize;
        let partial_tx = ((scale_tx - full_tx as f64) * 8.0).round() as usize;

        let block_bar = format!("{}{}", "█".repeat(full_block), blocks_chars[partial_block]);
        let tx_bar = format!("{}{}", "█".repeat(full_tx), blocks_chars[partial_tx]);

        println!(
            "{:>6} | {:<width$} | {:<width$}",
            blocks[i],
            block_bar,
            tx_bar,
            width = bar_max_width
        );
    }
}

/// Calculate block time averages
pub async fn calculate_block_averages(
    client: &Client,
    block_count: usize,
) -> Result<BlockAverages> {
    if block_count < 2 {
        return Err(anyhow!("At least 2 blocks required"));
    }

    let height = client.get_height().await?;
    let start = height.saturating_sub(block_count);
    let mut timestamps = Vec::with_capacity(block_count);

    for h in start..height {
        let block = client
            .get_block_by_height(h)
            .await?
            .ok_or_else(|| anyhow!("Block {} missing", h))?;
        timestamps.push(block.timestamp as f64);
    }

    if timestamps.len() < 2 {
        return Err(anyhow!("Not enough blocks"));
    }

    let mut deltas: Vec<f64> = timestamps.windows(2).map(|w| w[1] - w[0]).collect();

    let count = deltas.len() as f64;
    let average = deltas.iter().sum::<f64>() / count;
    let variance = deltas.iter().map(|d| (d - average).powi(2)).sum::<f64>() / count;
    let std_dev = variance.sqrt();
    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if deltas.len() % 2 == 0 {
        let mid = deltas.len() / 2;
        (deltas[mid - 1] + deltas[mid]) / 2.0
    } else {
        deltas[deltas.len() / 2]
    };
    let min = *deltas.first().unwrap();
    let max = *deltas.last().unwrap();

    Ok(BlockAverages {
        average,
        std_dev,
        median,
        min,
        max,
        _sample_size: deltas.len(),
    })
}

/// Calculate all blockchain stats
pub async fn calculate_chain_stats(client: &Client, block_count: usize) -> Result<ChainStats> {
    let block_time = calculate_block_averages(client, block_count).await?;

    let height = client.get_height().await?;
    let start = height.saturating_sub(block_count);

    let mut total_txs = 0usize;
    let mut total_io = 0usize;
    let mut total_size = 0usize;

    let mut miner_count: HashMap<[u8; 32], usize> = HashMap::new();
    let mut address_count: HashMap<[u8; 32], usize> = HashMap::new();

    let mut block_diffs = Vec::new();
    let mut tx_diffs = Vec::new();

    let mut first_ts = None;
    let mut last_ts = None;

    for h in start..height {
        let block = client
            .get_block_by_height(h)
            .await?
            .ok_or_else(|| anyhow!("Missing block {}", h))?;
        first_ts.get_or_insert(block.timestamp);
        last_ts = Some(block.timestamp);

        total_txs += block.transactions.len();
        for tx in &block.transactions {
            total_io += tx.inputs.len() + tx.outputs.len();
            for i in &tx.inputs {
                *address_count.entry(*i.output_owner.dump_buf()).or_default() += 1;
            }
            for o in &tx.outputs {
                *address_count.entry(*o.receiver.dump_buf()).or_default() += 1;
            }
        }

        if let Some(coinbase) = block.transactions.iter().filter(|tx| tx.inputs.len() == 0).collect::<Vec<&Transaction>>().first() {
            let out = coinbase.outputs[1];
            *miner_count.entry(*out.receiver.dump_buf()).or_default() += 1;
        }

        total_size += encode_to_vec(&block, bincode::config::standard())?.len();
        block_diffs.push(normalize_difficulty(&block.meta.block_pow_difficulty));
        tx_diffs.push(normalize_difficulty(&block.meta.tx_pow_difficulty));
    }

    let blocks_f = block_count as f64;
    let duration = (last_ts.unwrap() - first_ts.unwrap()) as f64;

    Ok(ChainStats {
        block_time,
        avg_txs_per_block: total_txs as f64 / blocks_f,
        avg_io_per_block: total_io as f64 / blocks_f,
        avg_block_size_bytes: total_size as f64 / blocks_f,
        tps: total_txs as f64 / duration,
        avg_block_difficulty: block_diffs.iter().sum::<f64>() / block_diffs.len() as f64,
        avg_tx_difficulty: tx_diffs.iter().sum::<f64>() / tx_diffs.len() as f64,
        top_miners: top_n(miner_count, 10),
        top_addresses: top_n(address_count, 10),
        block_difficulty_series: block_diffs,
        tx_difficulty_series: tx_diffs,
    })
}
