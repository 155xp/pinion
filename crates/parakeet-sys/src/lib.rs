//! Safe wrapper over whisper.cpp's Parakeet speech recogniser.
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

extern "C" {
    fn pk_load(path: *const c_char, use_gpu: c_int) -> *mut c_void;
    fn pk_transcribe(ctx: *mut c_void, samples: *const f32, n: c_int, threads: c_int) -> *mut c_char;
    fn pk_free_text(text: *mut c_char);
    fn pk_free(ctx: *mut c_void);
    fn pk_system_info() -> *const c_char;
}

/// The sample rate Parakeet expects.
pub const SAMPLE_RATE: u32 = 16_000;

/// A loaded model. Not thread safe: wrap it in a mutex to share it.
pub struct Model(*mut c_void);

// The context is only ever used from one thread at a time (behind a Mutex).
unsafe impl Send for Model {}

impl Model {
    pub fn load(path: &Path, use_gpu: bool) -> Result<Model, String> {
        let p = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let ctx = unsafe { pk_load(p.as_ptr(), use_gpu as c_int) };
        if ctx.is_null() {
            Err(format!("Couldn't load the speech model at {}", path.display()))
        } else {
            Ok(Model(ctx))
        }
    }

    /// Transcribes 16 kHz mono audio in `[-1, 1]`.
    pub fn transcribe(&mut self, samples: &[f32], threads: usize) -> Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let n = c_int::try_from(samples.len()).map_err(|_| "Audio too long.".to_string())?;
        let t = unsafe { pk_transcribe(self.0, samples.as_ptr(), n, threads.max(1) as c_int) };
        if t.is_null() {
            return Err("Transcription failed.".into());
        }
        let text = unsafe { CStr::from_ptr(t) }.to_string_lossy().into_owned();
        unsafe { pk_free_text(t) };
        Ok(text)
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        unsafe { pk_free(self.0) };
    }
}

pub fn system_info() -> String {
    unsafe { CStr::from_ptr(pk_system_info()) }.to_string_lossy().into_owned()
}
