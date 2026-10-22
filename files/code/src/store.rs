use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SYNC_META: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_meta");
const OFFLINE_QUEUE: TableDefinition<&str, u64> = TableDefinition::new("offline_queue");

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileSyncState {
    pub file_path: String,
    pub blake3_hash: String,
    pub notion_page_id: Option<String>,
    pub last_synced_ts: u64,
}

pub struct LocalStore {
    db: Database,
}

impl LocalStore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::create(db_path)?;
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(SYNC_META)?;
            let _ = write_txn.open_table(OFFLINE_QUEUE)?;
        }
        write_txn.commit()?;
        Ok(Self { db })
    }

    pub fn get_state(&self, path: &str) -> Result<Option<FileSyncState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SYNC_META)?;
        if let Some(bytes) = table.get(path)? {
            let state: FileSyncState = serde_json::from_slice(bytes.value())?;
            return Ok(Some(state));
        }
        Ok(None)
    }

    pub fn upsert_state(&self, state: &FileSyncState) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SYNC_META)?;
            let bytes = serde_json::to_vec(state)?;
            table.insert(state.file_path.as_str(), bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn enqueue_offline(&self, path: &str, timestamp: u64) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(OFFLINE_QUEUE)?;
            table.insert(path, timestamp)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn dequeue_offline(&self, path: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(OFFLINE_QUEUE)?;
            table.remove(path)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn list_queued(&self) -> Result<Vec<(String, u64)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(OFFLINE_QUEUE)?;
        let mut items = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            items.push((k.value().to_string(), v.value()));
        }
        Ok(items)
    }
}
