use crate::model::Library;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct Storage {
    dir: PathBuf,
    _lock: File,
}
impl Storage {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("Cannot create {}", dir.display()))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("instance.lock"))?;
        lock.try_lock_exclusive()
            .context("Another Interstellar FM instance is using this data directory")?;
        Ok(Self {
            dir: dir.to_owned(),
            _lock: lock,
        })
    }
    pub fn load(&self, limit: usize) -> Result<Library> {
        let path = self.dir.join("library.json");
        if !path.exists() {
            return Ok(Library::default());
        }
        let mut library: Library = serde_json::from_slice(&fs::read(&path)?).with_context(|| format!("Corrupt {} — preserved; restore library.json.bak or move the corrupt file aside", path.display()))?;
        if library.version != 1 {
            bail!(
                "Unsupported library schema {} — file left untouched",
                library.version
            );
        }
        library.volume = library.volume.min(100);
        let mut ids = vec![];
        library.favorites.retain(|s| {
            if ids.contains(&s.id) {
                false
            } else {
                ids.push(s.id.clone());
                true
            }
        });
        ids.clear();
        library.history.retain(|h| {
            if ids.contains(&h.station.id) {
                false
            } else {
                ids.push(h.station.id.clone());
                true
            }
        });
        library.history.truncate(limit);
        Ok(library)
    }
    pub fn save(&self, library: &Library) -> Result<()> {
        let path = self.dir.join("library.json");
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        temp.write_all(&serde_json::to_vec_pretty(library)?)?;
        temp.as_file().sync_all()?;
        if path.exists() {
            fs::copy(&path, self.dir.join("library.json.bak"))?;
        }
        temp.persist(&path)
            .context("Unable to replace library.json")?;
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_and_locks() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        assert!(Storage::open(dir.path()).is_err());
        let library = Library {
            volume: 42,
            ..Library::default()
        };
        storage.save(&library).unwrap();
        assert_eq!(storage.load(30).unwrap().volume, 42);
        storage.save(&Library::default()).unwrap();
        assert!(dir.path().join("library.json.bak").exists());
        drop(storage);
        assert!(Storage::open(dir.path()).is_ok());
    }
    #[test]
    fn vote_and_click_snapshots_survive_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let mut station = crate::model::Station::custom(
            url::Url::parse("https://example.com/live").unwrap(),
            "Radio",
            None,
        );
        station.votes = Some(12345);
        station.clicks = Some(67890);
        let mut library = Library::default();
        library.add(station.clone());
        library.record(station, 30);
        storage.save(&library).unwrap();
        drop(storage);
        let library = Storage::open(dir.path()).unwrap().load(30).unwrap();
        assert_eq!(
            (library.favorites[0].votes, library.favorites[0].clicks),
            (Some(12345), Some(67890))
        );
        assert_eq!(
            (
                library.history[0].station.votes,
                library.history[0].station.clicks
            ),
            (Some(12345), Some(67890))
        );
    }

    #[test]
    fn corrupt_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("library.json"), b"broken").unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        assert!(storage.load(30).is_err());
        assert_eq!(
            fs::read(dir.path().join("library.json")).unwrap(),
            b"broken"
        );
    }
}
