//! Every dictation, newest last, one JSON object per line.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEEP: usize = 500;

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub id: u64,
    /// When it was said, in ms since the epoch.
    pub at: u64,
    pub text: String,
    /// How long the speech was, in ms.
    pub ms: u64,
}

pub fn file(dir: &Path) -> PathBuf {
    dir.join("voice-history.jsonl")
}

pub fn add(dir: &Path, e: &Entry) {
    let _ = fs::create_dir_all(dir);
    let p = file(dir);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        if let Ok(line) = serde_json::to_string(e) {
            let _ = writeln!(f, "{line}");
        }
    }
    // Trim now and then so the file stays small.
    if fs::metadata(&p).map(|m| m.len() > 600_000).unwrap_or(false) {
        let all = read_all(dir);
        let keep = &all[all.len().saturating_sub(KEEP)..];
        write_all(dir, keep);
    }
}

fn read_all(dir: &Path) -> Vec<Entry> {
    fs::read_to_string(file(dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn write_all(dir: &Path, entries: &[Entry]) {
    let body: String = entries.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
    let _ = crate::vault::write_atomic(&file(dir), body.as_bytes());
}

/// Newest first.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut all = read_all(dir);
    all.reverse();
    all.truncate(KEEP);
    all
}

pub fn delete(dir: &Path, id: u64) {
    let all: Vec<Entry> = read_all(dir).into_iter().filter(|e| e.id != id).collect();
    write_all(dir, &all);
}

pub fn clear(dir: &Path) {
    let _ = fs::remove_file(file(dir));
}
