//! Persistent config: system config dir, cross-platform (recent file paths).

use std::path::PathBuf;

const CONFIG_DIR_NAME: &str = "h264bsanalyzer";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct AppConfig {
    /// Recent file paths, max 10, newest first.
    #[serde(default)]
    pub recent_paths: Vec<String>,
}

/// Config file path: config_dir/h264bsanalyzer/config.json.
fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME))
}

/// Load config from disk; default if missing or parse error.
pub fn load_config() -> AppConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return AppConfig::default(),
    };
    let Ok(s) = std::fs::read_to_string(&path) else {
        return AppConfig::default();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

/// Write config to disk; create config dir if needed.
pub fn save_config(config: &AppConfig) {
    let path = match config_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(config).unwrap_or_else(|_| "{}".to_string()),
    );
}
