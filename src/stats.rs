//! Per-song practice records (best tempo, best accuracy, times finished),
//! kept in `stats.json` next to the settings file.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SongStats {
    /// Times played through to the end in a practice mode.
    pub finished: u32,
    /// Highest tempo (%) a run was finished at.
    pub best_tempo: u16,
    /// Best share of right notes in a finished run, in %.
    pub best_accuracy: u8,
}

pub struct Stats {
    path: Option<PathBuf>,
    songs: Mutex<BTreeMap<String, SongStats>>,
}

impl Stats {
    /// Loads `path`; a missing or unreadable file starts empty.
    pub fn load(path: Option<PathBuf>) -> Self {
        let songs = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Stats { path, songs: Mutex::new(songs) }
    }

    pub fn all(&self) -> BTreeMap<String, SongStats> {
        self.songs.lock().unwrap().clone()
    }

    /// Records a finished run and saves. Returns the song's new record.
    pub fn record(&self, song: &str, tempo: u16, accuracy: u8) -> SongStats {
        let mut songs = self.songs.lock().unwrap();
        let s = songs.entry(song.to_string()).or_default();
        s.finished += 1;
        s.best_tempo = s.best_tempo.max(tempo);
        s.best_accuracy = s.best_accuracy.max(accuracy);
        let out = *s;
        if let Some(p) = &self.path {
            // Write-then-rename, like the settings: a power cut can't truncate it.
            let tmp = p.with_extension("json.tmp");
            let saved = serde_json::to_vec_pretty(&*songs)
                .map_err(|e| e.to_string())
                .and_then(|j| std::fs::write(&tmp, j).and_then(|_| std::fs::rename(&tmp, p)).map_err(|e| e.to_string()));
            if let Err(e) = saved {
                eprintln!("stats: can't save {}: {e}", p.display());
            }
        }
        out
    }

    /// Forgets a deleted song.
    pub fn remove(&self, song: &str) {
        self.songs.lock().unwrap().remove(song);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_best_and_survives_a_reload() {
        let path = std::env::temp_dir().join(format!("plv-stats-{}.json", std::process::id()));
        let s = Stats::load(Some(path.clone()));
        s.record("a.mid", 80, 90);
        let r = s.record("a.mid", 70, 95);
        assert_eq!(r, SongStats { finished: 2, best_tempo: 80, best_accuracy: 95 });
        assert_eq!(Stats::load(Some(path.clone())).all()["a.mid"], r);
        let _ = std::fs::remove_file(path);
    }
}
