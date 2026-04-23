use std::path::PathBuf;

pub struct IsolatedHome {
    _temp: tempfile::TempDir,
    pub path: PathBuf,
    pub rigor_dir: PathBuf,
}

impl IsolatedHome {
    pub fn new() -> Self {
        todo!()
    }

    pub fn write_rigor_yaml(&self, _content: &str) -> PathBuf {
        todo!()
    }

    pub fn home_str(&self) -> String {
        todo!()
    }

    pub fn rigor_dir_str(&self) -> String {
        todo!()
    }
}

impl Default for IsolatedHome {
    fn default() -> Self { Self::new() }
}
