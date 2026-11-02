//! The speech model: loaded when someone starts talking, freed after a quiet spell.

use parakeet_sys::Model;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IDLE_UNLOAD: Duration = Duration::from_secs(240);

pub struct Engine {
    path: PathBuf,
    model: Mutex<Option<Model>>,
    last_used: Mutex<Instant>,
}

impl Engine {
    pub fn new(path: PathBuf) -> Arc<Engine> {
        let e = Arc::new(Engine { path, model: Mutex::new(None), last_used: Mutex::new(Instant::now()) });
        let weak = Arc::downgrade(&e);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            let Some(e) = weak.upgrade() else { break };
            if e.last_used.lock().unwrap().elapsed() > IDLE_UNLOAD {
                if let Ok(mut m) = e.model.try_lock() {
                    *m = None;
                }
            }
        });
        e
    }

    fn threads() -> usize {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8)
    }

    /// Starts loading in the background so the model is warm by the time speech ends.
    pub fn preload(self: &Arc<Self>) {
        *self.last_used.lock().unwrap() = Instant::now();
        let me = self.clone();
        std::thread::spawn(move || {
            let mut m = me.model.lock().unwrap();
            if m.is_none() {
                *m = Model::load(&me.path, cfg!(all(target_os = "macos", target_arch = "aarch64"))).ok();
            }
        });
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let mut m = self.model.lock().unwrap();
        if m.is_none() {
            *m = Some(Model::load(&self.path, cfg!(all(target_os = "macos", target_arch = "aarch64")))?);
        }
        *self.last_used.lock().unwrap() = Instant::now();
        let out = m.as_mut().unwrap().transcribe(samples, Self::threads());
        *self.last_used.lock().unwrap() = Instant::now();
        out
    }

    pub fn unload(&self) {
        *self.model.lock().unwrap() = None;
    }
}
