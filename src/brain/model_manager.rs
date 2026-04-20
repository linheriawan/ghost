//! Model discovery — lists available GGUF files from known locations.
//!
//! Search order:
//!   1. `models/` directory relative to the current working directory
//!   2. Directory of the running executable
//!   3. Current working directory

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
}

pub struct ModelManager;

impl ModelManager {
    /// Return all `.gguf` files found in the standard search locations.
    pub fn list_available() -> Vec<ModelEntry> {
        let mut entries = Vec::new();

        let search_dirs = [
            // Preferred: `models/` next to cwd
            std::env::current_dir()
                .ok()
                .map(|d| d.join("models"))
                .unwrap_or_default(),
            // Fallback: next to the executable
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_default(),
            // Last resort: cwd itself
            std::env::current_dir().unwrap_or_default(),
        ];

        for dir in &search_dirs {
            let Ok(rd) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|x| x.to_str()) != Some("gguf") {
                    continue;
                }
                // Skip duplicates (same path already added from an earlier dir).
                if entries.iter().any(|e: &ModelEntry| e.path == path) {
                    continue;
                }
                let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                entries.push(ModelEntry { path, name, size_bytes });
            }
        }

        entries
    }

    /// Return a human-readable file size string (e.g. "1.2 GB").
    pub fn format_size(bytes: u64) -> String {
        const GB: u64 = 1_000_000_000;
        const MB: u64 = 1_000_000;
        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.0} MB", bytes as f64 / MB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}
