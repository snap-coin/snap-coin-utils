use anyhow::anyhow;
use clap::{Parser, Subcommand};
use num_bigint::BigUint;
use num_traits::cast::ToPrimitive;
use snap_coin::{
    api::client::Client,
    blockchain_data_provider::BlockchainDataProvider,
    core::transaction::TransactionId,
    crypto::{Hash, keys::Public},
    to_snap,
};
use tokio::net::lookup_host;

mod averages;

pub fn normalize_difficulty(target: &[u8; 32]) -> f64 {
    let target = BigUint::from_bytes_be(target);
    let max_target = BigUint::from_bytes_be(&[255u8; 32]);

    let max_f = max_target.to_f64().unwrap(); // ~1e77
    let target_f = target.to_f64().unwrap();

    max_f / target_f
}

pub fn format_biguint_hr(value: &[u8; 32]) -> String {
    let units = ["", "K", "M", "G", "T", "P"];
    let thousand = 1000.0;

    // Use normalize_difficulty to get f64
    let mut value_f = normalize_difficulty(value);

    // Format with units
    let mut unit_index = 0;
    while value_f >= thousand && unit_index < units.len() - 1 {
        value_f /= thousand;
        unit_index += 1;
    }

    // Keep 2 decimal places if not an integer
    if value_f.fract() == 0.0 {
        format!("{}{}", value_f as u64, units[unit_index])
    } else {
        format!("{:.2}{}", value_f, units[unit_index])
    }
}

#[derive(Parser)]
#[command(
    name = "snap-coin-stats",
    version,
    about = "Read snap coin blockchain and node data from the command line"
)]
struct Cli {
    /// Node address to connect too
    node: String,

    /// Sub commands
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get block by height or hash
    Block {
        /// Block height (number) or block hash (base36)
        id: String,
    },

    /// Get transaction by hash (base36)
    Tx {
        /// Transaction hash (base36)
        id: String,
    },

    /// Get address (base36) info
    Addr {
        /// Address (base36)
        address: String,
    },

    /// Get current blockchain height
    Height,

    /// Get current difficulty
    Difficulty,

    /// Get Current Mempool
    Mempool,

    /// Calculate basic average info for the past X blocks
    Averages { blocks: usize },
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Cli::parse();
    let mut nodes = match lookup_host(args.node.clone()).await {
        Ok(node) => node,
        Err(..) => {
            return Err(anyhow!("Could not resolve {}", args.node));
        }
    };
    let client = Client::connect(nodes.next().unwrap()).await?;
    match args.command {
        Commands::Block { id } => {
            let height: Option<usize> = id.parse().ok();
            let hash = Hash::new_from_base36(&id);

            if let Some(height) = height {
                println!("{:#?}", client.get_block_by_height(height).await?);
            } else if let Some(hash) = hash {
                println!("{:#?}", client.get_block_by_hash(hash).await?);
            } else {
                return Err(anyhow!(
                    "Block identifier {id} is not valid. Expected base36 hash or height."
                ));
            }
        }
        Commands::Tx { id } => {
            let tx_id = TransactionId::new_from_base36(&id);
            if let Some(tx_id) = tx_id {
                println!("{:#?}", client.get_transaction(&tx_id).await?);
            } else {
                return Err(anyhow!(
                    "Transaction identifier {id} is not valid. Expected base36 transaction id"
                ));
            }
        }
        Commands::Addr { address } => {
            let public = Public::new_from_base36(&address);
            if let Some(public) = public {
                println!(
                    "Balance: {:#?} SNAP",
                    to_snap(client.get_balance(public).await?)
                );
                let utxos = client.get_available_transaction_outputs(public).await?;
                println!("Available UTXOS:\n{:#?}", utxos);
                // println!("{}", to_snap(utxos.iter().fold(0, |acc, utxo| acc + utxo.1.amount)));
                println!(
                    "Transaction history (blocks):\n{:?}",
                    client.get_transactions_of_address(public).await?
                );
            } else {
                return Err(anyhow!(
                    "Public address {address} is not valid. Expected base36 address"
                ));
            }
        }
        Commands::Height => println!("Height: {}", client.get_height().await?),
        Commands::Difficulty => {
            println!(
                "Block Difficulty: {}",
                format_biguint_hr(&client.get_block_difficulty().await?)
            );
            println!(
                "Transaction Difficulty: {}",
                format_biguint_hr(&client.get_transaction_difficulty().await?)
            );
        }
        Commands::Mempool => {
            println!("Mempool:\n{:#?}", client.get_mempool().await?);
        }
        Commands::Averages { blocks } => {
            let stats = averages::calculate_chain_stats(&client, blocks).await?;
            let height = client.get_height().await?;

            // Plot block times
            let block_numbers: Vec<usize> =
                (height - stats.tx_difficulty_series.len()..height).collect();
            averages::plot_difficulties(
                &block_numbers,
                &stats.block_difficulty_series,
                &stats.tx_difficulty_series,
            );

            // Optional: print top miners & addresses
            println!("\nTop 10 Miners:");
            for (addr, count) in &stats.top_miners {
                println!(
                    "{} -> {} blocks",
                    Public::new_from_buf(addr).dump_base36(),
                    count
                );
            }

            println!("\nTop 10 Addresses:");
            for (addr, count) in &stats.top_addresses {
                println!(
                    "{} -> {} appearances",
                    Public::new_from_buf(addr).dump_base36(),
                    count
                );
            }

            println!(
                "\nAvg TXs/block: {:.2}, Avg IO/block: {:.2}, Avg block size: {:.2} bytes, TPS: {:.2}",
                stats.avg_txs_per_block,
                stats.avg_io_per_block,
                stats.avg_block_size_bytes,
                stats.tps
            );

            println!(
                "Avg Block Difficulty: {:.2}, Avg TX Difficulty: {:.2}",
                stats.avg_block_difficulty, stats.avg_tx_difficulty
            );

            println!(
                "Block Time Avg: {:.2}s, Median: {:.2}s, Std Dev: {:.2}s, Min: {:.2}s, Max: {:.2}s",
                stats.block_time.average,
                stats.block_time.median,
                stats.block_time.std_dev,
                stats.block_time.min,
                stats.block_time.max
            );
        }
    }

    Ok(())
}
