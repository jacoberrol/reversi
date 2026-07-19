//! Tiny persisted config: remembers the last username between launches so the
//! login screen can pre-fill it. The password is never stored.

use std::path::PathBuf;

fn username_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "reversi")?;
    Some(dirs.config_dir().join("username"))
}

/// The remembered username, or empty if none.
pub fn load_username() -> String {
    username_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Remember `name` for next launch (best effort — ignores I/O errors).
pub fn save_username(name: &str) {
    let Some(path) = username_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, name);
}
