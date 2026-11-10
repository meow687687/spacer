use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScorerWeights {
    #[serde(default = "default_size_weight")]
    pub size: f64,
    #[serde(default = "default_age_weight")]
    pub age: f64,
    #[serde(default = "default_category_weight")]
    pub category: f64,
}

fn default_size_weight() -> f64 {
    0.45
}
fn default_age_weight() -> f64 {
    0.30
}
fn default_category_weight() -> f64 {
    0.25
}

impl Default for ScorerWeights {
    fn default() -> Self {
        Self {
            size: 0.45,
            age: 0.30,
            category: 0.25,
        }
    }
}

impl ScorerWeights {
    /// Returns normalized weights summing to 1.0
    pub fn normalized(&self) -> (f64, f64, f64) {
        let total = self.size + self.age + self.category;
        if total <= 0.0 {
            (0.45, 0.30, 0.25)
        } else {
            (self.size / total, self.age / total, self.category / total)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpacerConfig {
    #[serde(default)]
    pub weights: ScorerWeights,
    #[serde(default)]
    pub extra_protected_paths: Vec<PathBuf>,
}

impl SpacerConfig {
    /// Loads configuration from default XDG path: ~/.config/spacer/config.toml
    pub fn load_default() -> Self {
        let config_path = get_default_config_path();
        if let Some(ref path) = config_path
            && path.exists()
            && let Ok(content) = std::fs::read_to_string(path)
            && let Ok(cfg) = toml::from_str::<SpacerConfig>(&content)
        {
            return cfg;
        }
        Self::default()
    }

    /// Loads configuration from a specific file path
    #[allow(dead_code)]
    pub fn load_from_file(path: &Path) -> Result<Self, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        let cfg: SpacerConfig = toml::from_str(&content)?;
        Ok(cfg)
    }

    /// Loads configuration from a TOML string
    #[allow(dead_code)]
    pub fn load_from_str(s: &str) -> Result<Self, anyhow::Error> {
        let cfg: SpacerConfig = toml::from_str(s)?;
        Ok(cfg)
    }
}

/// Resolves standard config path for spacer
pub fn get_default_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("spacer").join("config.toml"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home).join(".config").join("spacer").join("config.toml"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_weights() {
        let weights = ScorerWeights::default();
        let (w_size, w_age, w_cat) = weights.normalized();
        assert!((w_size - 0.45).abs() < 1e-4);
        assert!((w_age - 0.30).abs() < 1e-4);
        assert!((w_cat - 0.25).abs() < 1e-4);
    }

    #[test]
    fn test_custom_weights_parse() {
        let toml_str = r#"
        [weights]
        size = 0.60
        age = 0.20
        category = 0.20
        "#;
        let config = SpacerConfig::load_from_str(toml_str).unwrap();
        assert_eq!(config.weights.size, 0.60);
        assert_eq!(config.weights.age, 0.20);
        assert_eq!(config.weights.category, 0.20);
    }
}
