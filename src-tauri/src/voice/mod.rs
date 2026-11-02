//! Pinion Voice: hold a key to talk into any app, tap it to keep listening,
//! or record a meeting straight into a note. Everything runs on this computer.

mod audio;
mod engine;
mod history;
mod meeting;
mod model;
mod paste;
mod platform;
mod text;

use crate::settings::Store;
use crate::{show_main, Vault};
use engine::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

/// Logical size of the transparent window the pill lives in.
const PILL_W: f64 = 340.0;
const PILL_H: f64 = 76.0;
/// Space under the capsule inside that window, for its shadow.
const PILL_BOTTOM: f64 = 12.0;
/// A press shorter than this locks listening on instead of push-to-talk.
const TAP: Duration = Duration::from_millis(350);
const MAX_DICTATION: Duration = Duration::from_secs(600);

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceSettings {
    pub shortcut: String,
    pub microphone: Option<String>,
    pub system_audio: bool,
    pub remove_fillers: bool,
    pub show_pill: bool,
    pub launch_at_login: bool,
    pub meeting_folder: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            shortcut: default_shortcut().into(),
            microphone: None,
            system_audio: true,
            remove_fillers: true,
            show_pill: true,
            launch_at_login: false,
            meeting_folder: "Meetings".into(),
        }
    }
}

fn default_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "Alt+Space"
    } else {
        "Ctrl+Shift+Space"
    }
}

enum Dictation {
    Idle,
    Listening {
        id: u64,
        capture: Option<audio::Capture>,
        tap: Option<Arc<audio::Tap>>,
        hands_free: bool,
        pressed: Instant,
    },
    Working,
}

pub struct Voice {
    app: AppHandle,
    dir: PathBuf,
    engine: Arc<Engine>,
    dictation: Mutex<Dictation>,
    meeting: Mutex<Option<meeting::Meeting>>,
    next_id: AtomicU64,
    release_gen: AtomicU64,
    pump: AtomicBool,
    download: Mutex<Option<Arc<AtomicBool>>>,
    progress: Mutex<Option<(u64, u64)>>,
    shortcut_error: Mutex<Option<String>>,
    escape: AtomicBool,
    pill_area: Mutex<(f64, f64)>,
    tray_meeting: Mutex<Option<MenuItem<Wry>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MeetingInfo {
    path: String,
    started_ms: u64,
    system_audio: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VoiceState {
    ready: bool,
    model: &'static str,
    model_size: u64,
    downloading: Option<(u64, u64)>,
    settings: VoiceSettings,
    shortcut_label: String,
    shortcut_error: Option<String>,
    meeting: Option<MeetingInfo>,
    accessibility: Option<bool>,
    /// idle, listening, handsfree or working
    dictation: &'static str,
    tint: String,
    platform: &'static str,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Voice {
    fn settings(&self) -> VoiceSettings {
        let raw = self.app.state::<Store>().get().voice;
        serde_json::from_value(Value::Object(raw)).unwrap_or_default()
    }

    fn ready(&self) -> bool {
        model::is_ready(&self.dir.join("models"))
    }

    pub fn state(&self) -> VoiceState {
        let s = self.settings();
        let dictation = match &*self.dictation.lock().unwrap() {
            Dictation::Idle => "idle",
            Dictation::Listening { hands_free: true, .. } => "handsfree",
            Dictation::Listening { .. } => "listening",
            Dictation::Working => "working",
        };
        let meeting = self.meeting.lock().unwrap().as_ref().map(|m| MeetingInfo {
            path: m.path.clone(),
            started_ms: m.started_ms,
            system_audio: m.has_system_audio(),
        });
        let tint = self
            .app
            .state::<Store>()
            .get()
            .ui
            .get("theme")
            .and_then(|t| t.get("tint"))
            .and_then(|t| t.as_str())
            .unwrap_or("paper")
            .to_string();
        VoiceState {
            ready: self.ready(),
            model: model::NAME,
            model_size: model::SIZE,
            downloading: *self.progress.lock().unwrap(),
            shortcut_label: platform::shortcut_label(&s.shortcut),
            settings: s,
            shortcut_error: self.shortcut_error.lock().unwrap().clone(),
            meeting,
            accessibility: platform::accessibility(false),
            dictation,
            tint,
            platform: crate::PLATFORM,
        }
    }

    fn broadcast(&self) {
        let _ = self.app.emit("voice-state", self.state());
    }

    fn notice(&self, msg: &str) {
        let _ = self.app.emit_to("pill", "pill-notice", msg);
    }

    // ---------- dictation ----------

    fn on_press(self: &Arc<Self>) {
        self.release_gen.fetch_add(1, Ordering::SeqCst);
        let what = match &*self.dictation.lock().unwrap() {
            Dictation::Idle => 0,
            Dictation::Listening { hands_free: true, .. } => 1,
            _ => 2,
        };
        match what {
            0 => self.start(false),
            1 => self.finish(),
            _ => {}
        }
    }

    fn on_release(self: &Arc<Self>) {
        let gen = self.release_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        std::thread::spawn(move || {
            // X11 repeats a held key as release-press pairs; a quick press means it's still held.
            if cfg!(target_os = "linux") {
                std::thread::sleep(Duration::from_millis(45));
            }
            if me.release_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            let action = match &mut *me.dictation.lock().unwrap() {
                Dictation::Listening { hands_free, pressed, .. } if !*hands_free => {
                    if pressed.elapsed() < TAP {
                        *hands_free = true;
                        1
                    } else {
                        2
                    }
                }
                _ => 0,
            };
            match action {
                1 => {
                    me.set_escape(true);
                    me.broadcast();
                }
                2 => me.finish(),
                _ => {}
            }
        });
    }

    pub fn toggle(self: &Arc<Self>) {
        let what = match &*self.dictation.lock().unwrap() {
            Dictation::Idle => 0,
            Dictation::Listening { .. } => 1,
            Dictation::Working => 2,
        };
        match what {
            0 => self.start(true),
            1 => self.finish(),
            _ => {}
        }
    }

    fn start(self: &Arc<Self>, hands_free: bool) {
        if !self.ready() {
            self.notice("Set up voice in Pinion first");
            let _ = show_main(&self.app);
            let _ = self.app.emit("show-voice", ());
            return;
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut d = self.dictation.lock().unwrap();
            if !matches!(*d, Dictation::Idle) {
                return;
            }
            *d = Dictation::Listening { id, capture: None, tap: None, hands_free, pressed: Instant::now() };
        }
        if hands_free {
            self.set_escape(true);
        }
        self.engine.preload();
        place_pill(&self.app);
        self.broadcast();
        let me = self.clone();
        let mic = self.settings().microphone;
        std::thread::spawn(move || {
            let res = audio::Capture::start(audio::Source::Mic, mic.as_deref());
            let mut d = me.dictation.lock().unwrap();
            let current = matches!(&*d, Dictation::Listening { id: cur, .. } if *cur == id);
            match res {
                Ok(c) if current => {
                    if let Dictation::Listening { capture, tap, .. } = &mut *d {
                        *tap = Some(c.tap.clone());
                        *capture = Some(c);
                    }
                    drop(d);
                    me.ensure_pump();
                    me.watchdog(id);
                }
                Err(e) if current => {
                    *d = Dictation::Idle;
                    drop(d);
                    me.set_escape(false);
                    me.broadcast();
                    me.notice(&e);
                }
                _ => {}
            }
        });
    }

    /// Stops a dictation that has run far too long.
    fn watchdog(self: &Arc<Self>, id: u64) {
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(MAX_DICTATION);
            if matches!(&*me.dictation.lock().unwrap(), Dictation::Listening { id: cur, .. } if *cur == id) {
                me.finish();
            }
        });
    }

    fn cancel(self: &Arc<Self>) {
        {
            let mut d = self.dictation.lock().unwrap();
            if !matches!(*d, Dictation::Listening { .. }) {
                return;
            }
            *d = Dictation::Idle;
        }
        self.set_escape(false);
        self.broadcast();
    }

    fn finish(self: &Arc<Self>) {
        let capture = {
            let mut d = self.dictation.lock().unwrap();
            match std::mem::replace(&mut *d, Dictation::Working) {
                Dictation::Listening { capture, .. } => capture,
                other => {
                    *d = other;
                    return;
                }
            }
        };
        self.set_escape(false);
        self.broadcast();
        let me = self.clone();
        std::thread::spawn(move || {
            let samples = capture.map(|c| c.stop()).unwrap_or_default();
            let ms = (samples.len() as u64 * 1000) / audio::RATE as u64;
            let heard = match audio::trim_silence(&samples) {
                Some(speech) => me.engine.transcribe(speech),
                None => Ok(String::new()),
            };
            *me.dictation.lock().unwrap() = Dictation::Idle;
            me.broadcast();
            match heard {
                Ok(raw) => {
                    let words = text::clean(&raw, me.settings().remove_fillers);
                    if words.is_empty() {
                        me.notice("Didn't catch that");
                        return;
                    }
                    let outcome = paste::paste(&words);
                    let entry = history::Entry { id: now_ms(), at: now_ms(), text: words, ms };
                    history::add(&me.dir, &entry);
                    let _ = me.app.emit("voice-history", &entry);
                    match outcome {
                        Ok(paste::Pasted::Typed) => {}
                        Ok(paste::Pasted::Copied) => {
                            me.notice(if cfg!(target_os = "macos") { "Copied. Paste with ⌘ V" } else { "Copied. Paste with Ctrl V" })
                        }
                        Err(e) => me.notice(&e),
                    }
                }
                Err(e) => me.notice(&e),
            }
        });
    }

    /// Escape cancels while listening hands-free.
    fn set_escape(&self, on: bool) {
        if self.escape.swap(on, Ordering::SeqCst) == on {
            return;
        }
        let esc = Shortcut::new(None, Code::Escape);
        let gs = self.app.global_shortcut();
        let _ = if on { gs.register(esc) } else { gs.unregister(esc) };
    }

    /// Streams loudness to the pill while anything is listening.
    fn ensure_pump(self: &Arc<Self>) {
        if self.pump.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(33));
                let dict = match &*me.dictation.lock().unwrap() {
                    Dictation::Listening { tap: Some(t), .. } => Some(t.level()),
                    _ => None,
                };
                let meet = me.meeting.lock().unwrap().as_ref().map(|m| {
                    let other = m.system.as_ref().map(|s| s.level()).unwrap_or(0.0);
                    m.mic.level().max(other)
                });
                if dict.is_none() && meet.is_none() {
                    break;
                }
                let _ = me.app.emit_to("pill", "pill-level", (dict, meet));
            }
            me.pump.store(false, Ordering::SeqCst);
            let active = matches!(&*me.dictation.lock().unwrap(), Dictation::Listening { tap: Some(_), .. })
                || me.meeting.lock().unwrap().is_some();
            if active {
                me.ensure_pump();
            }
        });
    }

    // ---------- meetings ----------

    pub fn toggle_meeting(self: &Arc<Self>) -> Result<(), String> {
        let running = self.meeting.lock().unwrap().take();
        if let Some(m) = running {
            m.stop();
            self.notice("Saving the meeting note…");
            self.tray_text(false);
            self.broadcast();
            return Ok(());
        }
        if !self.ready() {
            let _ = show_main(&self.app);
            let _ = self.app.emit("show-voice", ());
            return Err("Set up voice first.".into());
        }
        let root = self.app.state::<Vault>().root()?;
        let s = self.settings();
        let folder = crate::vault::clean_name(&s.meeting_folder).unwrap_or_else(|_| "Meetings".into());
        let opts = meeting::Options {
            root,
            folder,
            mic: s.microphone.clone(),
            system_audio: s.system_audio,
            remove_fillers: s.remove_fillers,
        };
        let app = self.app.clone();
        let app2 = self.app.clone();
        let m = meeting::Meeting::start(
            self.engine.clone(),
            opts,
            move |path| {
                let _ = app.emit("note-updated", path);
            },
            move |path| {
                let _ = app2.emit("note-updated", path);
                let _ = app2.emit("vault-changed", ());
                let _ = app2.emit_to("pill", "pill-notice", "Meeting saved to your notes");
                if let Some(v) = app2.try_state::<Arc<Voice>>() {
                    v.broadcast();
                }
            },
        )?;
        let path = m.path.clone();
        let both = m.has_system_audio();
        *self.meeting.lock().unwrap() = Some(m);
        self.engine.preload();
        self.ensure_pump();
        self.tray_text(true);
        place_pill(&self.app);
        self.broadcast();
        let _ = self.app.emit("vault-changed", ());
        let _ = self.app.emit("meeting-started", &path);
        if s.system_audio && !both {
            self.notice("Recording your microphone only");
        }
        Ok(())
    }

    fn tray_text(&self, recording: bool) {
        if let Some(item) = self.tray_meeting.lock().unwrap().as_ref() {
            let _ = item.set_text(if recording { "Stop recording" } else { "Record a meeting" });
        }
    }

    // ---------- model ----------

    fn start_download(self: &Arc<Self>) {
        let mut d = self.download.lock().unwrap();
        if d.is_some() || self.ready() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *d = Some(cancel.clone());
        *self.progress.lock().unwrap() = Some((0, model::SIZE));
        drop(d);
        self.broadcast();
        let me = self.clone();
        std::thread::spawn(move || {
            let dir = me.dir.join("models");
            let res = model::download(&dir, &cancel, |done, total| {
                *me.progress.lock().unwrap() = Some((done, total));
                let _ = me.app.emit("voice-download", (done, total));
            });
            *me.download.lock().unwrap() = None;
            *me.progress.lock().unwrap() = None;
            match res {
                Ok(()) => {
                    let label = platform::shortcut_label(&me.settings().shortcut);
                    me.notice(&format!("Voice is ready. Hold {label} to talk"));
                }
                Err(e) if e == "cancelled" => {}
                Err(e) => {
                    let _ = me.app.emit("voice-error", e);
                }
            }
            me.broadcast();
        });
    }

    fn register_shortcut(&self, s: &str) {
        let gs = self.app.global_shortcut();
        let _ = gs.unregister_all();
        self.escape.store(false, Ordering::SeqCst);
        let err = match s.parse::<Shortcut>() {
            Ok(sc) => gs.register(sc).err().map(|_| {
                format!("{} is already used by another app. Pick another shortcut.", platform::shortcut_label(s))
            }),
            Err(_) => Some(format!("{s} isn't a shortcut Pinion understands.")),
        };
        *self.shortcut_error.lock().unwrap() = err;
    }
}

// ---------- the pill ----------

fn place_pill(app: &AppHandle) {
    let Some(w) = app.get_webview_window("pill") else { return };
    let cursor = app.cursor_position().ok();
    let monitor = cursor
        .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let area = m.work_area();
    let sf = m.scale_factor();
    // Use the real size: some Linux setups won't make a window as short as asked.
    let (pw, ph) = match w.outer_size() {
        Ok(s) if s.width > 0 => (s.width as i32, s.height as i32),
        _ => ((PILL_W * sf) as i32, (PILL_H * sf) as i32),
    };
    let x = area.position.x + (area.size.width as i32 - pw) / 2;
    let y = area.position.y + area.size.height as i32 - ph;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

fn create_pill(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "pill", WebviewUrl::App("pill.html".into()))
        .title("Pinion Voice")
        .inner_size(PILL_W, PILL_H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .visible_on_all_workspaces(true)
        .visible(false)
        .build()?;
    place_pill(app);
    // Linux needs the window realised before it can let clicks through.
    w.show()?;
    let _ = w.set_ignore_cursor_events(true);
    place_pill(app);
    Ok(())
}

/// The pill window is mostly transparent: only the capsule itself takes clicks.
fn watch_hover(voice: Arc<Voice>) {
    std::thread::spawn(move || {
        let mut inside = false;
        let mut last_size = (0, 0);
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let Some(w) = voice.app.get_webview_window("pill") else { continue };
            let (Ok(pos), Ok(size), Ok(sf), Ok(c)) =
                (w.outer_position(), w.outer_size(), w.scale_factor(), voice.app.cursor_position())
            else {
                continue;
            };
            // The window can settle at a different size than asked (GTK, DPI changes).
            if (size.width, size.height) != last_size {
                last_size = (size.width, size.height);
                place_pill(&voice.app);
                continue;
            }
            let (aw, ah) = *voice.pill_area.lock().unwrap();
            let pad = 4.0 * sf;
            let cx = pos.x as f64 + size.width as f64 / 2.0;
            let bottom = pos.y as f64 + size.height as f64 - PILL_BOTTOM * sf;
            let now = c.x >= cx - aw * sf / 2.0 - pad
                && c.x <= cx + aw * sf / 2.0 + pad
                && c.y >= bottom - ah * sf - pad
                && c.y <= bottom + pad;
            if now != inside {
                inside = now;
                let _ = w.set_ignore_cursor_events(!now);
                let _ = voice.app.emit_to("pill", "pill-hover", now);
            }
        }
    });
}

fn create_tray(app: &AppHandle, voice: &Arc<Voice>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Pinion", true, None::<&str>)?;
    let meeting = MenuItem::with_id(app, "meeting", "Record a meeting", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Pinion", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &meeting, &PredefinedMenuItem::separator(app)?, &quit])?;
    *voice.tray_meeting.lock().unwrap() = Some(meeting);
    // macOS tints a black template to suit the menu bar; elsewhere a dark taskbar
    // would swallow it, so use the app icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray.png");
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::include_image!("icons/32x32.png");
    TrayIconBuilder::with_id("pinion")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Pinion")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => {
                let _ = show_main(app);
            }
            "meeting" => {
                if let Some(v) = app.try_state::<Arc<Voice>>() {
                    if let Err(e) = v.toggle_meeting() {
                        v.notice(&e);
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

// ---------- wiring ----------

pub fn plugins(b: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    b.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, shortcut, event| {
                let Some(v) = app.try_state::<Arc<Voice>>() else { return };
                let v = v.inner().clone();
                if shortcut.key == Code::Escape && shortcut.mods.is_empty() {
                    if event.state() == ShortcutState::Pressed {
                        v.cancel();
                    }
                    return;
                }
                match event.state() {
                    ShortcutState::Pressed => v.on_press(),
                    ShortcutState::Released => v.on_release(),
                }
            })
            .build(),
    )
    .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--background"])))
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let dir = app.path().app_data_dir()?;
    let engine = Engine::new(model::path(&dir.join("models")));
    let voice = Arc::new(Voice {
        app: app.clone(),
        dir,
        engine,
        dictation: Mutex::new(Dictation::Idle),
        meeting: Mutex::new(None),
        next_id: AtomicU64::new(0),
        release_gen: AtomicU64::new(0),
        pump: AtomicBool::new(false),
        download: Mutex::new(None),
        progress: Mutex::new(None),
        shortcut_error: Mutex::new(None),
        escape: AtomicBool::new(false),
        pill_area: Mutex::new((56.0, 10.0)),
        tray_meeting: Mutex::new(None),
    });
    app.manage(voice.clone());
    voice.register_shortcut(&voice.settings().shortcut);
    if let Err(e) = create_pill(app) {
        eprintln!("pill: {e}");
    }
    if let Err(e) = create_tray(app, &voice) {
        eprintln!("tray: {e}");
    }
    watch_hover(voice);
    Ok(())
}

pub fn main_closed(app: &AppHandle) {
    platform::dock(app, false);
}

pub fn main_opened(app: &AppHandle) {
    platform::dock(app, true);
}

pub fn is_background_launch(args: &[String]) -> bool {
    args.iter().any(|a| a == "--background")
}

/// `pinion --dictate` and `pinion --meeting`, for desktops where global shortcuts
/// don't reach apps (Wayland): bind these commands to a key instead.
pub fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    let Some(v) = app.try_state::<Arc<Voice>>() else { return false };
    if args.iter().any(|a| a == "--dictate") {
        v.toggle();
        return true;
    }
    if args.iter().any(|a| a == "--meeting") {
        if let Err(e) = v.toggle_meeting() {
            v.notice(&e);
        }
        return true;
    }
    is_background_launch(args)
}

// ---------- commands ----------

type V<'a> = tauri::State<'a, Arc<Voice>>;

#[tauri::command]
pub fn voice_state(v: V<'_>) -> VoiceState {
    v.state()
}

#[tauri::command]
pub fn voice_settings(app: AppHandle, v: V<'_>, patch: Map<String, Value>) -> VoiceState {
    let before = v.settings();
    app.state::<Store>().update(|s| crate::settings::merge(&mut s.voice, patch));
    let after = v.settings();
    if after.shortcut != before.shortcut {
        v.register_shortcut(&after.shortcut);
    }
    if after.launch_at_login != before.launch_at_login {
        let al = app.autolaunch();
        let _ = if after.launch_at_login { al.enable() } else { al.disable() };
    }
    v.broadcast();
    v.state()
}

#[tauri::command(async)]
pub fn voice_history(v: V<'_>) -> Vec<history::Entry> {
    history::list(&v.dir)
}

#[tauri::command(async)]
pub fn voice_history_delete(v: V<'_>, id: u64) {
    history::delete(&v.dir, id);
}

#[tauri::command(async)]
pub fn voice_history_clear(v: V<'_>) {
    history::clear(&v.dir);
}

#[tauri::command]
pub fn voice_model_download(v: V<'_>) {
    v.start_download();
}

#[tauri::command]
pub fn voice_model_cancel(v: V<'_>) {
    if let Some(c) = v.download.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

#[tauri::command(async)]
pub fn voice_model_delete(v: V<'_>) {
    v.engine.unload();
    model::remove(&v.dir.join("models"));
    v.broadcast();
}

#[tauri::command(async)]
pub fn voice_devices() -> Vec<audio::DeviceInfo> {
    audio::input_devices()
}

#[tauri::command]
pub fn voice_toggle(v: V<'_>) {
    v.inner().toggle();
}

#[tauri::command]
pub fn voice_cancel(v: V<'_>) {
    v.inner().cancel();
}

#[tauri::command]
pub fn meeting_toggle(v: V<'_>) -> Result<(), String> {
    v.inner().toggle_meeting()
}

/// The pill tells us how big its capsule is, so clicks outside it fall through.
#[tauri::command]
pub fn pill_area(v: V<'_>, width: f64, height: f64) {
    *v.pill_area.lock().unwrap() = (width, height);
}

#[tauri::command]
pub fn voice_permissions(request: bool) -> Option<bool> {
    platform::accessibility(request)
}

#[tauri::command]
pub fn voice_copy(text: String) {
    paste::copy(&text);
}
