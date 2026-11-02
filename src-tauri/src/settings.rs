//! Settings live in one JSON file in the app's config folder. The UI owns the
//! shape of `ui`; the voice module owns `voice`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub vault: Option<String>,
    pub recent: Vec<String>,
    pub ui: Map<String, Value>,
    pub voice: Map<String, Value>,
    pub window: Option<WindowState>,
    /// Page colour of the current theme, used before the UI has painted.
    pub background: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub maximized: bool,
}

pub struct Store {
    path: PathBuf,
    data: Mutex<Settings>,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Store { path, data: Mutex::new(data) }
    }

    pub fn get(&self) -> Settings {
        self.data.lock().unwrap().clone()
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let mut d = self.data.lock().unwrap();
        let r = f(&mut d);
        if let Ok(bytes) = serde_json::to_vec_pretty(&*d) {
            let _ = crate::vault::write_atomic(&self.path, &bytes);
        }
        r
    }
}

pub fn merge(into: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (k, v) in patch {
        if v.is_null() {
            into.remove(&k);
        } else {
            into.insert(k, v);
        }
    }
}

/// `#rrggbb` to a Tauri colour.
pub fn parse_hex(s: &str) -> Option<tauri::window::Color> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(tauri::window::Color((n >> 16) as u8, (n >> 8) as u8, n as u8, 255))
}
