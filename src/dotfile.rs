use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub package_managers: HashMap<String, PackageManager>,
    pub tools: HashMap<String, Tool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageManager {
    pub description: Option<String>,
    pub platform: String,
    pub check: Option<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    pub description: Option<String>,

    pub deps: Option<Vec<String>>,

    pub tags: Option<Vec<String>>,

    pub check: Option<String>,

    pub install: Option<HashMap<String, Vec<String>>>,

    pub configs: Option<Config>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub description: Option<String>,
    pub source: String,
    pub targets: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn parse_toml() {
        let dotpath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema.toml");

        let text: String = fs::read_to_string(dotpath).unwrap();
        let manifest = toml::from_str::<Manifest>(&text);

        assert!(manifest.is_ok());
    }
}
