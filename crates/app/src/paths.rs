use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&data_dir);
        Self { data_dir }
    }

    pub fn default_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ado-planner")
    }

    pub fn prefs_file(&self) -> PathBuf {
        self.data_dir.join("prefs.json")
    }

    pub fn cache_file(&self) -> PathBuf {
        self.data_dir.join("query-cache.bin")
    }
}
