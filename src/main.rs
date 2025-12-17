use anyhow::anyhow;
use clap::{Parser, Subcommand};
use num_bigint::BigUint;
use snap_coin::{
    api::client::Client,
    blockchain_data_provider::BlockchainDataProvider,
    core::{transaction::TransactionId, utils::max_256_bui},
    crypto::{Hash, keys::Public},
    to_snap,
};
use tokio::net::lookup_host;

fn format_biguint(mut value: BigUint) -> String {
    let units = ["", "K", "M", "G", "T", "P"];
    let thousand = BigUint::from(1000u32);

    let mut unit_index = 0;
    while value >= thousand && unit_index < units.len() - 1 {
        value /= &thousand;
        unit_index += 1;
    }

    format!("{}{}", value, units[unit_index])
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
    Mempool
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
                println!("{:#?}", client.get_block_by_hash(&hash).await?);
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
            let block_diff = BigUint::from_bytes_be(&client.get_block_difficulty().await?);
            let h_block_diff = max_256_bui() / &block_diff;

            let transaction_diff = BigUint::from_bytes_be(&client.get_transaction_difficulty().await?);
            let h_transaction_diff = max_256_bui() / &transaction_diff;

            println!("Block Difficulty: {}", format_biguint(h_block_diff));
            println!("Transaction Difficulty: {}", format_biguint(h_transaction_diff));
        }
        Commands::Mempool => {
            println!("Mempool:\n{:#?}", client.get_mempool().await?);
        }
    }

    Ok(())
}
