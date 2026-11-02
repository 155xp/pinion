//! Records a meeting into a note: your microphone and, when allowed, the other side
//! of the call, cut at pauses, transcribed as it goes and appended to the note.

use super::audio::{self, Capture, Source};
use super::engine::Engine;
use super::text;
use crate::vault;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const FRAME: usize = 480; // 30 ms at 16 kHz
const MIN_CHUNK: usize = 2 * 16_000;
const MAX_CHUNK: usize = 26 * 16_000;
const PAUSE_FRAMES: usize = 20; // 0.6 s of quiet ends a phrase

pub struct Meeting {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub path: String,
    pub started_ms: u64,
    pub mic: Arc<audio::Tap>,
    pub system: Option<Arc<audio::Tap>>,
}

pub struct Options {
    pub root: PathBuf,
    pub folder: String,
    pub mic: Option<String>,
    pub system_audio: bool,
    pub remove_fillers: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Speaker {
    Me,
    Others,
}

struct Chunker {
    who: Speaker,
    buf: Vec<f32>,
    /// Seconds into the meeting of `buf[0]`.
    start: f64,
}

struct Chunk {
    who: Speaker,
    start: f64,
    samples: Vec<f32>,
}

impl Chunker {
    fn new(who: Speaker) -> Self {
        Chunker { who, buf: Vec::new(), start: 0.0 }
    }

    /// Takes the next finished phrase, if there is one.
    fn next(&mut self, flush: bool) -> Option<Option<Chunk>> {
        if self.buf.is_empty() || (!flush && self.buf.len() < MIN_CHUNK) {
            return None;
        }
        let levels = audio::frame_levels(&self.buf);
        let thr = audio::speech_threshold(&levels);
        let mut cut = None;
        let mut quiet = 0;
        for (i, l) in levels.iter().enumerate().skip(MIN_CHUNK / FRAME) {
            if *l < thr {
                quiet += 1;
                if quiet >= PAUSE_FRAMES {
                    cut = Some((i - PAUSE_FRAMES / 2) * FRAME);
                    break;
                }
            } else {
                quiet = 0;
            }
        }
        if cut.is_none() && self.buf.len() >= MAX_CHUNK {
            // Nobody paused: cut at the quietest moment of the last few seconds.
            let from = (MAX_CHUNK - 4 * 16_000) / FRAME;
            let to = (MAX_CHUNK / FRAME).min(levels.len());
            let quietest = (from..to).min_by(|a, b| levels[*a].partial_cmp(&levels[*b]).unwrap()).unwrap_or(to - 1);
            cut = Some(quietest * FRAME);
        }
        if cut.is_none() && flush {
            cut = Some(self.buf.len());
        }
        let cut = cut?;
        let samples: Vec<f32> = self.buf.drain(..cut).collect();
        let start = self.start;
        self.start += cut as f64 / audio::RATE as f64;
        let levels = &levels[..(cut / FRAME).min(levels.len())];
        let loud = levels.iter().filter(|l| **l > thr).count();
        if loud < 10 {
            return Some(None);
        }
        let first = levels.iter().position(|l| *l > thr).unwrap_or(0);
        let skip = first.saturating_sub(5) * FRAME;
        Some(Some(Chunk {
            who: self.who,
            start: start + skip as f64 / audio::RATE as f64,
            samples: samples[skip.min(samples.len())..].to_vec(),
        }))
    }
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

struct Writer {
    root: PathBuf,
    path: String,
    labelled: bool,
    last: Option<(Speaker, f64)>,
}

impl Writer {
    fn append(&mut self, who: Speaker, at: f64, words: &str) -> bool {
        let Ok(note) = vault::read(&self.root, &self.path) else { return false };
        let mut text = note.text.trim_end().to_string();
        let same = matches!(self.last, Some((w, t)) if w == who && at - t < 45.0);
        if same {
            text.push(' ');
            text.push_str(words);
        } else {
            let head = if self.labelled {
                format!("**{}** · {}", if who == Speaker::Me { "You" } else { "Others" }, clock(at))
            } else {
                format!("**{}**", clock(at))
            };
            text.push_str(&format!("\n\n{head}\n{words}"));
        }
        text.push('\n');
        self.last = Some((who, at));
        vault::write(&self.root, &self.path, &text).is_ok()
    }

    fn finish(&self, secs: f64) {
        let Ok(note) = vault::read(&self.root, &self.path) else { return };
        let minutes = ((secs / 60.0).round() as u64).max(1);
        let text = note.text.replacen("duration: recording\n", &format!("duration: {minutes} min\n"), 1);
        let _ = vault::write(&self.root, &self.path, &text);
    }
}

impl Meeting {
    pub fn start(
        engine: Arc<Engine>,
        opts: Options,
        on_update: impl Fn(&str) + Send + 'static,
        on_done: impl FnOnce(&str) + Send + 'static,
    ) -> Result<Meeting, String> {
        let now = chrono::Local::now();
        let name = format!("{} Meeting", now.format("%Y-%m-%d %H.%M"));
        let mic = Capture::start(Source::Mic, opts.mic.as_deref())?;
        let system = if opts.system_audio { Capture::start(Source::System, None).ok() } else { None };
        let labelled = system.is_some();
        let body = format!(
            "---\ntype: meeting\ndate: {}\nduration: recording\n---\n",
            now.format("%Y-%m-%d %H:%M")
        );
        let path = vault::create_note(&opts.root, &opts.folder, &name, &body)?;
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let started_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mic_tap = mic.tap.clone();
        let system_tap = system.as_ref().map(|s| s.tap.clone());

        let flag = stop.clone();
        let note = path.clone();
        let root = opts.root.clone();
        let fillers = opts.remove_fillers;
        let worker = std::thread::Builder::new()
            .name("pinion-meeting".into())
            .spawn(move || {
                let mut writer = Writer { root, path: note.clone(), labelled, last: None };
                let mut me = Chunker::new(Speaker::Me);
                let mut them = Chunker::new(Speaker::Others);
                let mut mic = Some(mic);
                let mut system = system;
                loop {
                    let stopping = flag.load(Ordering::Relaxed);
                    if !stopping {
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    if let Some(m) = &mic {
                        me.buf.extend(m.tap.take());
                    }
                    if let Some(s) = &system {
                        them.buf.extend(s.tap.take());
                    }
                    if stopping {
                        if let Some(m) = mic.take() {
                            me.buf.extend(m.stop());
                        }
                        if let Some(s) = system.take() {
                            them.buf.extend(s.stop());
                        }
                    }
                    let mut ready: Vec<Chunk> = Vec::new();
                    for c in [&mut me, &mut them] {
                        while let Some(next) = c.next(stopping) {
                            if let Some(chunk) = next {
                                ready.push(chunk);
                            }
                        }
                    }
                    ready.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
                    for chunk in ready {
                        let Ok(raw) = engine.transcribe(&chunk.samples) else { continue };
                        let words = text::clean(&raw, fillers);
                        if !words.is_empty() && writer.append(chunk.who, chunk.start, &words) {
                            on_update(&note);
                        }
                    }
                    if stopping {
                        break;
                    }
                }
                writer.finish(started.elapsed().as_secs_f64());
                on_done(&note);
            })
            .map_err(|e| e.to_string())?;

        Ok(Meeting { stop, worker: Some(worker), path, started_ms, mic: mic_tap, system: system_tap })
    }

    /// Stops listening. The last words are written in the background.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            std::thread::spawn(move || {
                let _ = w.join();
            });
        }
    }

    pub fn has_system_audio(&self) -> bool {
        self.system.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(secs: f32) -> Vec<f32> {
        (0..(secs * 16_000.0) as usize).map(|i| (i as f32 * 0.07).sin() * 0.3).collect()
    }

    #[test]
    fn cuts_at_pauses() {
        let mut c = Chunker::new(Speaker::Me);
        c.buf.extend(speech(3.0));
        c.buf.extend(vec![0.0005; 16_000]);
        c.buf.extend(speech(1.0));
        let first = c.next(false).unwrap().unwrap();
        assert!(first.samples.len() > 3 * 16_000 && first.samples.len() < 4 * 16_000);
        assert!(c.next(false).is_none());
        let last = c.next(true).unwrap().unwrap();
        assert!(last.start > 3.0);
        assert_eq!(clock(3725.0), "1:02:05");
    }
}
