use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeterMode {
    #[default]
    Bands,
    Loudness,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mpv_path: String,
    pub audio_meter: bool,
    pub audio_meter_mode: MeterMode,
    pub history_limit: usize,
    pub debounce_ms: u64,
    pub api_base: Option<String>,
    pub accent: String,
    pub keybindings: BTreeMap<String, Vec<String>>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mpv_path: "mpv".into(),
            audio_meter: true,
            audio_meter_mode: MeterMode::default(),
            history_limit: 30,
            debounce_ms: 350,
            api_base: None,
            accent: "magenta".into(),
            keybindings: BTreeMap::new(),
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let config: Self = if path.exists() {
            toml::from_str(&std::fs::read_to_string(path)?)
                .with_context(|| format!("Invalid config: {}", path.display()))?
        } else {
            Self::default()
        };
        if !(1..=10000).contains(&config.history_limit) {
            bail!("history_limit must be 1–10000");
        }
        if !(50..=5000).contains(&config.debounce_ms) {
            bail!("debounce_ms must be 50–5000");
        }
        if let Some(base) = &config.api_base {
            let url = url::Url::parse(base)?;
            if url.scheme() != "https" {
                bail!("api_base must use HTTPS");
            }
        }
        crate::input::Keymap::new(&config.keybindings)?;
        if !matches!(
            config.accent.as_str(),
            "magenta" | "cyan" | "green" | "blue" | "yellow" | "red"
        ) {
            bail!("accent must be magenta, cyan, green, blue, yellow, or red");
        }
        Ok(config)
    }
}
pub fn user_dir(variable: &str, fallback: &str) -> Result<PathBuf> {
    let base = env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
        .context("Set HOME or an XDG directory variable")?;
    Ok(base.join("interstellarfm-cli"))
}
