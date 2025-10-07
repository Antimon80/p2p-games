use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub peer_id: String,
    pub nickname: String,
    pub current_room_topic_hex: Option<String>,
    pub current_room_host_addr: Option<String>,
}

impl SessionState {
    fn storage_path() -> PathBuf {
        let mut path = dirs::data_local_dir().unwrap_or(std::env::temp_dir());
        path.push("p2p-games");
        let _ = fs::create_dir_all(&path);
        path.push("session.json");
        path
    }

    pub fn load() -> std::io::Result<Self> {
        let path = Self::storage_path();
        if path.exists() {
            let b = fs::read(path)?;
            Ok(serde_json::from_slice(&b)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).unwrap();
        fs::write(Self::storage_path(), json)
    }
}
