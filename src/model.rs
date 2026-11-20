use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", content = "id", rename_all = "snake_case")]
pub enum StationId {
    RadioBrowser(String),
    Custom(Uuid),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Station {
    pub id: StationId,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub playlist_source: Option<String>,
}

impl Station {
    pub fn custom(url: Url, name: &str, source: Option<String>) -> Self {
        let name = if name.trim().is_empty() {
            url.host_str().unwrap_or("Custom station").to_owned()
        } else {
            clean(name.trim())
        };
        Self {
            id: StationId::Custom(Uuid::new_v4()),
            name,
            url: url.to_string(),
            country: String::new(),
            codec: String::new(),
            bitrate: 0,
            tags: String::new(),
            language: String::new(),
            homepage: String::new(),
            playlist_source: source,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub station: Station,
    pub last_played_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub version: u32,
    pub favorites: Vec<Station>,
    pub history: Vec<HistoryEntry>,
    pub volume: u8,
    pub muted: bool,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            favorites: vec![],
            history: vec![],
            volume: 70,
            muted: false,
        }
    }
}

impl Library {
    pub fn is_saved(&self, station: &Station) -> bool {
        self.favorites.iter().any(|s| s.id == station.id)
    }

    pub fn remove(&mut self, id: &StationId) {
        self.favorites.retain(|s| &s.id != id);
    }

    /// Reuse custom identities from history when the same URL is imported again.
    pub fn add(&mut self, mut station: Station) -> bool {
        if matches!(station.id, StationId::Custom(_)) {
            if self
                .favorites
                .iter()
                .any(|s| matches!(s.id, StationId::Custom(_)) && same_url(&s.url, &station.url))
            {
                return false;
            }
            if let Some(entry) = self.history.iter().find(|h| {
                matches!(h.station.id, StationId::Custom(_))
                    && same_url(&h.station.url, &station.url)
            }) {
                station.id = entry.station.id.clone();
            }
        }
        if self.is_saved(&station) {
            return false;
        }
        self.favorites.push(station);
        true
    }

    pub fn record(&mut self, station: Station, limit: usize) {
        self.history.retain(|h| h.station.id != station.id);
        self.history.insert(
            0,
            HistoryEntry {
                station,
                last_played_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        );
        self.history.truncate(limit);
    }
}

pub fn same_url(a: &str, b: &str) -> bool {
    match (Url::parse(a), Url::parse(b)) {
        (Ok(mut a), Ok(mut b)) => {
            a.set_fragment(None);
            b.set_fragment(None);
            a == b
        }
        _ => a == b,
    }
}

/// Strip ANSI sequences as well as standalone terminal control characters.
pub fn clean(text: &str) -> String {
    strip_ansi_escapes::strip_str(text)
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn station(path: &str) -> Station {
        Station::custom(
            Url::parse(&format!("https://example.com/{path}")).unwrap(),
            path,
            None,
        )
    }
    #[test]
    fn favorites_and_history_are_independent() {
        let mut lib = Library::default();
        let s = station("live");
        assert!(lib.add(s.clone()));
        assert!(!lib.add(s.clone()));
        lib.record(s.clone(), 30);
        lib.remove(&s.id);
        assert!(lib.favorites.is_empty());
        assert_eq!(lib.history.len(), 1);
        assert!(lib.add(s.clone()));
        assert_eq!(lib.favorites[0].id, s.id);
    }
    #[test]
    fn history_deduplicates_and_caps() {
        let mut lib = Library::default();
        let a = station("a");
        lib.record(a.clone(), 2);
        lib.record(station("b"), 2);
        lib.record(a.clone(), 2);
        assert_eq!(lib.history.len(), 2);
        assert_eq!(lib.history[0].station.id, a.id);
        lib.record(station("c"), 2);
        assert_eq!(lib.history.len(), 2);
    }
    #[test]
    fn custom_import_reuses_history_identity() {
        let mut lib = Library::default();
        let a = station("a");
        lib.record(a.clone(), 30);
        assert!(lib.add(station("a")));
        assert_eq!(lib.favorites[0].id, a.id);
    }
    #[test]
    fn url_normalization_preserves_tokens() {
        assert!(same_url(
            "https://EXAMPLE.com:443/live",
            "https://example.com/live"
        ));
        assert!(!same_url(
            "https://example.com/?key=1",
            "https://example.com/?key=2"
        ));
    }
    #[test]
    fn strips_terminal_escapes() {
        assert_eq!(clean("\x1b[31mRadio\x1b[0m\n\x07"), "Radio");
    }
}
