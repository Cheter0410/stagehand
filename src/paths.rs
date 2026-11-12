//! Folders Stagehand uses.

use std::path::PathBuf;

/// Settings and the cached catalog.
pub fn config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("stagehand"))
}

/// Downloads (deleted after installing).
pub fn download_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("stagehand")
        .join("downloads")
}

/// Linux: where apps installed by Stagehand live (one folder per app).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn linux_apps_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("stagehand").join("apps"))
}
