//! Microphone and system audio, captured on their own threads and turned into 16 kHz mono.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub const RATE: u32 = 16_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Mic,
    /// What the computer is playing: the other side of a call.
    System,
}

/// Audio collected so far, shared with the capture thread.
#[derive(Default)]
pub struct Tap {
    level: AtomicU32,
    buf: Mutex<Vec<f32>>,
    pub error: Mutex<Option<String>>,
}

impl Tap {
    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *self.buf.lock().unwrap())
    }
    /// Loudness of the last few milliseconds, 0..1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

pub struct Capture {
    pub tap: Arc<Tap>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub fn start(source: Source, device: Option<&str>) -> Result<Capture, String> {
        let tap = Arc::new(Tap::default());
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let t = tap.clone();
        let device = device.map(str::to_string);
        let thread = std::thread::Builder::new()
            .name("pinion-audio".into())
            .spawn(move || {
                let stream = match open(source, device.as_deref(), t) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(format!("Couldn't start recording: {e}")));
                    return;
                }
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(())) => Ok(Capture { tap, stop: Some(stop_tx), thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("The microphone didn't answer.".into()),
        }
    }

    /// Stops recording and returns whatever hasn't been taken yet.
    pub fn stop(mut self) -> Vec<f32> {
        self.halt();
        self.tap.take()
    }

    fn halt(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.halt();
    }
}

#[derive(Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

pub fn input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else { return vec![] };
    devices
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            let name = d.description().ok().map(|x| x.name().to_string()).unwrap_or_else(|| d.to_string());
            // Monitors of outputs aren't microphones.
            if is_monitor(&id, &name) {
                return None;
            }
            Some(DeviceInfo { id, name })
        })
        .collect()
}

fn is_monitor(id: &str, name: &str) -> bool {
    id.ends_with(".monitor") || name.starts_with("Monitor of")
}

fn find_input(host: &cpal::Host, id: &str) -> Option<cpal::Device> {
    let want: cpal::DeviceId = id.parse().ok()?;
    host.input_devices().ok()?.find(|d| d.id().ok().as_ref() == Some(&want))
}

/// The device that carries what the computer is playing.
fn system_device(host: &cpal::Host) -> Option<cpal::Device> {
    #[cfg(target_os = "linux")]
    {
        // PulseAudio and PipeWire expose each output's sound as a "monitor" source.
        let out = host.default_output_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
        let monitors: Vec<cpal::Device> = host
            .input_devices()
            .ok()?
            .filter(|d| {
                let id = d.id().map(|i| i.to_string()).unwrap_or_default();
                let name = d.description().map(|x| x.name().to_string()).unwrap_or_default();
                is_monitor(&id, &name)
            })
            .collect();
        if let Some(out) = out {
            let key = out.rsplit(':').next().unwrap_or(&out).to_string();
            if let Some(d) = monitors.iter().find(|d| d.id().map(|i| i.to_string().contains(&key)).unwrap_or(false)) {
                return Some(d.clone());
            }
        }
        monitors.into_iter().next()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // WASAPI and Core Audio record an output device in loopback when opened for input.
        host.default_output_device()
    }
}

fn open(source: Source, device: Option<&str>, tap: Arc<Tap>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let (dev, supported) = match source {
        Source::Mic => {
            let d = device
                .and_then(|id| find_input(&host, id))
                .or_else(|| host.default_input_device())
                .ok_or("No microphone was found.")?;
            let c = d.default_input_config().map_err(|e| format!("The microphone isn't available: {e}"))?;
            (d, c)
        }
        Source::System => {
            let d = system_device(&host).ok_or("This computer can't record its own sound.")?;
            let c = if cfg!(target_os = "linux") { d.default_input_config() } else { d.default_output_config() }
                .map_err(|e| format!("Couldn't record the computer's sound: {e}"))?;
            (d, c)
        }
    };
    let format = supported.sample_format();
    let config = supported.config();
    let channels = config.channels.max(1) as usize;
    let rate = config.sample_rate;
    let err_tap = tap.clone();
    let on_error = move |e: cpal::Error| {
        *err_tap.error.lock().unwrap() = Some(e.to_string());
    };
    macro_rules! build {
        ($t:ty) => {
            build_stream::<$t>(&dev, config, channels, rate, tap, on_error)
        };
    }
    let stream = match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I32 => build!(i32),
        SampleFormat::U16 => build!(u16),
        SampleFormat::U8 => build!(u8),
        SampleFormat::I8 => build!(i8),
        SampleFormat::F64 => build!(f64),
        SampleFormat::U32 => build!(u32),
        other => return Err(format!("This audio device uses an unsupported format ({other}).")),
    };
    stream.map_err(|e| format!("Couldn't open the audio device: {e}"))
}

fn build_stream<T>(
    dev: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    tap: Arc<Tap>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let mut resampler = Resampler::new(rate, RATE);
    let mut mono: Vec<f32> = Vec::with_capacity(4096);
    dev.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], _| {
            mono.clear();
            for frame in data.chunks(channels) {
                let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
                mono.push(sum / channels as f32);
            }
            if mono.is_empty() {
                return;
            }
            let rms = (mono.iter().map(|x| x * x).sum::<f32>() / mono.len() as f32).sqrt();
            tap.level.store(rms.to_bits(), Ordering::Relaxed);
            let out = resampler.process(&mono);
            tap.buf.lock().unwrap().extend_from_slice(&out);
        },
        on_error,
        None,
    )
}

/// Low-pass filter plus linear interpolation, carried across callback boundaries.
pub struct Resampler {
    ratio: f64,
    pos: f64,
    last: f32,
    taps: Vec<f32>,
    hist: Vec<f32>,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let ratio = from as f64 / to as f64;
        let taps = if ratio <= 1.0 {
            vec![1.0]
        } else {
            let n = 41usize;
            let cutoff = 0.45 / ratio; // cycles per input sample
            let m = (n - 1) as f64 / 2.0;
            let mut t: Vec<f32> = (0..n)
                .map(|i| {
                    let x = i as f64 - m;
                    let sinc = if x == 0.0 { 2.0 * cutoff } else { (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x) };
                    let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos()
                        + 0.08 * (4.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
                    (sinc * w) as f32
                })
                .collect();
            let sum: f32 = t.iter().sum();
            t.iter_mut().for_each(|v| *v /= sum);
            t
        };
        let hist = vec![0.0; taps.len() - 1];
        Resampler { ratio, pos: 0.0, last: 0.0, taps, hist }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.ratio - 1.0).abs() < 1e-9 {
            return input.to_vec();
        }
        let n = self.taps.len();
        let mut buf = Vec::with_capacity(self.hist.len() + input.len());
        buf.extend_from_slice(&self.hist);
        buf.extend_from_slice(input);
        let filtered: Vec<f32> = (0..input.len())
            .map(|k| buf[k..k + n].iter().zip(&self.taps).map(|(a, b)| a * b).sum())
            .collect();
        self.hist = buf[buf.len() - (n - 1)..].to_vec();
        let mut out = Vec::with_capacity((input.len() as f64 / self.ratio) as usize + 2);
        let len = filtered.len() as f64;
        while self.pos < len - 1.0 {
            let i = self.pos.floor();
            let f = (self.pos - i) as f32;
            let a = if i < 0.0 { self.last } else { filtered[i as usize] };
            let b = filtered[(i + 1.0) as usize];
            out.push(a + (b - a) * f);
            self.pos += self.ratio;
        }
        self.pos -= len;
        self.last = *filtered.last().unwrap_or(&0.0);
        out
    }
}

/// Root mean square of each 30 ms frame of 16 kHz audio.
pub fn frame_levels(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(480)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt())
        .collect()
}

/// A speech threshold that adapts to the room: a few times the quiet floor.
pub fn speech_threshold(levels: &[f32]) -> f32 {
    if levels.is_empty() {
        return 0.01;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 10];
    (floor * 2.5).clamp(0.005, 0.02)
}

/// Trims silence from both ends, keeping a little air. None if nothing was said.
pub fn trim_silence(samples: &[f32]) -> Option<&[f32]> {
    let levels = frame_levels(samples);
    let thr = speech_threshold(&levels);
    let loud: Vec<usize> = levels.iter().enumerate().filter(|(_, l)| **l > thr).map(|(i, _)| i).collect();
    if loud.len() < 5 {
        return None;
    }
    let first = loud[0].saturating_sub(8) * 480;
    let last = ((loud[loud.len() - 1] + 10) * 480).min(samples.len());
    Some(&samples[first..last])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_48k_to_16k_without_drift() {
        let mut r = Resampler::new(48_000, 16_000);
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.5).collect();
        let mut out = Vec::new();
        for chunk in tone.chunks(441) {
            out.extend(r.process(chunk));
        }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "{}", out.len());
        let peak = out[1000..].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.45 && peak < 0.55, "{peak}");
    }

    #[test]
    fn trims_quiet_edges() {
        let mut s = vec![0.0005f32; 16_000];
        s.extend((0..16_000).map(|i| (i as f32 * 0.05).sin() * 0.3));
        s.extend(vec![0.0005f32; 16_000]);
        let t = trim_silence(&s).unwrap();
        assert!(t.len() < 26_000 && t.len() > 16_000, "{}", t.len());
        assert!(trim_silence(&vec![0.0005f32; 16_000]).is_none());
    }
}
