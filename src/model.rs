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
    #[serde(
        default,
        rename = "countrycode",
        skip_serializing_if = "String::is_empty"
    )]
    pub country_code: String,
    // Read old library snapshots without relying on the deprecated API field.
    #[serde(default, rename = "country", skip_serializing_if = "String::is_empty")]
    pub legacy_country: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default)]
    pub votes: Option<u64>,
    #[serde(default)]
    pub clicks: Option<u64>,
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
            country_code: String::new(),
            legacy_country: String::new(),
            codec: String::new(),
            bitrate: 0,
            votes: None,
            clicks: None,
            tags: String::new(),
            language: String::new(),
            homepage: String::new(),
            playlist_source: source,
        }
    }

    pub fn country(&self) -> &str {
        if !self.country_code.is_empty() {
            &self.country_code
        } else if !self.legacy_country.is_empty() {
            &self.legacy_country
        } else {
            "—"
        }
    }

    /// Refresh directory counts/country code without changing saved names or identity.
    pub fn update_metadata_from(&mut self, station: &Self) -> bool {
        if self.id != station.id {
            return false;
        }
        let previous = (self.votes, self.clicks);
        if station.votes.is_some() {
            self.votes = station.votes;
        }
        if station.clicks.is_some() {
            self.clicks = station.clicks;
        }
        let mut changed = previous != (self.votes, self.clicks);
        if !station.country_code.is_empty() {
            changed |= self.country_code != station.country_code || !self.legacy_country.is_empty();
            self.country_code.clone_from(&station.country_code);
            self.legacy_country.clear();
        }
        changed
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
    fn legacy_and_custom_stations_have_unknown_counts() {
        let station = station("live");
        assert_eq!((station.votes, station.clicks), (None, None));
        let mut value = serde_json::to_value(&station).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("votes");
        object.remove("clicks");
        let loaded: Station = serde_json::from_value(value).unwrap();
        assert_eq!((loaded.votes, loaded.clicks), (None, None));
    }

    #[test]
    fn metadata_updates_preserve_identity_name_and_known_values() {
        let mut saved = station("live");
        let mut fresh = saved.clone();
        fresh.name = "Updated directory name".into();
        fresh.votes = Some(0);
        fresh.clicks = Some(12345);
        fresh.country_code = "DE".into();
        saved.legacy_country = "Germany".into();
        assert!(saved.update_metadata_from(&fresh));
        assert_eq!(saved.country(), "DE");
        assert!(saved.legacy_country.is_empty());
        assert_eq!(saved.name, "live");
        assert_eq!((saved.votes, saved.clicks), (Some(0), Some(12345)));
        assert!(!saved.update_metadata_from(&station("other")));
        fresh.votes = None;
        fresh.clicks = None;
        fresh.country_code.clear();
        assert!(!saved.update_metadata_from(&fresh));
        assert_eq!(saved.country(), "DE");
        assert_eq!((saved.votes, saved.clicks), (Some(0), Some(12345)));
    }

    #[test]
    fn country_only_refresh_marks_a_snapshot_changed_and_is_idempotent() {
        let mut saved = station("live");
        saved.legacy_country = "Germany".into();
        let mut fresh = saved.clone();
        fresh.country_code = "DE".into();
        fresh.legacy_country.clear();
        assert!(saved.update_metadata_from(&fresh));
        assert_eq!(saved.country(), "DE");
        assert!(saved.legacy_country.is_empty());
        assert!(!saved.update_metadata_from(&fresh));
    }

    #[test]
    fn old_country_snapshots_still_load_and_country_codes_take_precedence() {
        let mut value = serde_json::to_value(station("live")).unwrap();
        value["country"] = "Germany".into();
        let legacy: Station = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(legacy.country(), "Germany");
        assert!(legacy.country_code.is_empty());
        value["countrycode"] = "DE".into();
        let loaded: Station = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.country(), "DE");
        let mut current = station("live");
        assert_eq!(current.country(), "—");
        current.country_code = "GB".into();
        let value = serde_json::to_value(current).unwrap();
        assert_eq!(value["countrycode"], "GB");
        assert!(value.get("country").is_none());
    }

    #[test]
    fn strips_terminal_escapes() {
        assert_eq!(clean("\x1b[31mRadio\x1b[0m\n\x07"), "Radio");
    }
}
