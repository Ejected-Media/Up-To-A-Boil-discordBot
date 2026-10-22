mod notion;
mod store;

use anyhow::Result;
use clap::{Parser, Subcommand};
use notion::NotionClient;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use store::{FileSyncState, LocalStore};
use tabled::{Table, Tabled};

#[derive(Parser)]
#[command(name = "glox-sync", version, about = "Local-first Markdown <-> Notion sync utility")]
struct Cli {
    /// Path to local redb state file
    #[arg(long, default_value = ".glox_sync.redb")]
    db_path: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Push a local Markdown file to Notion (queues locally if offline)
    Push {
        /// Path to the .md file
        file: PathBuf,
        #[arg(long, env = "NOTION_API_KEY")]
        api_key: String,
        #[arg(long, env = "NOTION_PARENT_ID")]
        parent_id: String,
    },
    /// Inspect local sync state and offline queue
    Status,
    /// Flush all queued offline changes to Notion
    Flush {
        #[arg(long, env = "NOTION_API_KEY")]
        api_key: String,
        #[arg(long, env = "NOTION_PARENT_ID")]
        parent_id: String,
    },
}

#[derive(Tabled)]
struct QueueRow {
    #[tabled(rename = "Staged File Path")]
    path: String,
    #[tabled(rename = "Queued Timestamp (UTC)")]
    timestamp: u64,
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = LocalStore::open(&cli.db_path)?;

    match cli.command {
        Commands::Push { file, api_key, parent_id } => {
            sync_file(&store, &file, &api_key, &parent_id).await?;
        }
        Commands::Status => {
            let queued = store.list_queued()?;
            if queued.is_empty() {
                println!("✓ Offline queue is empty. All local files synced.");
            } else {
                let rows: Vec<QueueRow> = queued
                    .into_iter()
                    .map(|(path, timestamp)| QueueRow { path, timestamp })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
        Commands::Flush { api_key, parent_id } => {
            let queued = store.list_queued()?;
            if queued.is_empty() {
                println!("✓ Nothing to flush.");
                return Ok(());
            }
            for (path_str, _) in queued {
                let path = PathBuf::from(&path_str);
                println!("→ Flushing {}...", path_str);
                sync_file(&store, &path, &api_key, &parent_id).await?;
            }
        }
    }

    Ok(())
}

async fn sync_file(
    store: &LocalStore,
    file: &PathBuf,
    api_key: &str,
    parent_id: &str,
) -> Result<()> {
    let content = tokio::fs::read_to_string(file).await?;
    let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    let path_key = file.to_string_lossy().to_string();

    if let Some(existing) = store.get_state(&path_key)? {
        if existing.blake3_hash == hash {
            println!("✓ [SKIP] {} is unchanged (BLAKE3: {}..)", path_key, &hash[..8]);
            return Ok(());
        }
    }

    let title = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled Note");

    let client = NotionClient::new(api_key.to_string(), parent_id.to_string());

    match client.push_new_page(title, &content).await {
        Ok(page_id) => {
            let state = FileSyncState {
                file_path: path_key.clone(),
                blake3_hash: hash,
                notion_page_id: Some(page_id.clone()),
                last_synced_ts: now_ts(),
            };
            store.upsert_state(&state)?;
            store.dequeue_offline(&path_key)?;
            println!("✓ [SYNCED] {} -> Notion Page ID: {}", path_key, page_id);
        }
        Err(e) => {
            eprintln!("! [OFFLINE/ERR] Could not reach Notion: {}. Staging in redb queue.", e);
            store.enqueue_offline(&path_key, now_ts())?;
        }
    }

    Ok(())
}
