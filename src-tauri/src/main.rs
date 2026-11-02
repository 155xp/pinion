#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod links;
mod settings;
mod vault;
#[cfg(feature = "voice")]
mod voice;
mod welcome;

use serde::Serialize;
use serde_json::{Map, Value};
use settings::{parse_hex, Store, WindowState};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use vault::{Backlink, Entry, Hit, NoteText, Renamed};

pub struct Vault(Mutex<Option<PathBuf>>);

impl Vault {
    pub fn root(&self) -> Result<PathBuf, String> {
        self.0.lock().unwrap().clone().ok_or_else(|| "No vault is open.".to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultInfo {
    path: String,
    /// The path with the home folder shortened to `~`.
    display: String,
    name: String,
    entries: Vec<Entry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Boot {
    vault: VaultInfo,
    recent: Vec<String>,
    ui: Map<String, Value>,
    platform: &'static str,
    voice: bool,
    version: String,
}

pub(crate) const PLATFORM: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(windows) {
    "windows"
} else {
    "linux"
};

fn vault_info(root: &Path) -> VaultInfo {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let display = match home.as_deref().and_then(|h| root.strip_prefix(h).ok()) {
        Some(rest) => Path::new("~").join(rest).to_string_lossy().into_owned(),
        None => root.to_string_lossy().into_owned(),
    };
    VaultInfo {
        path: root.to_string_lossy().into_owned(),
        display,
        name: root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Vault".into()),
        entries: vault::list(root),
    }
}

fn default_vault(app: &AppHandle) -> PathBuf {
    let base = app.path().document_dir().or_else(|_| app.path().home_dir()).unwrap_or_else(|_| PathBuf::from("."));
    base.join("Pinion")
}

/// Points the app at a folder, remembers it, and lets the webview load its images.
fn use_vault(app: &AppHandle, root: &Path) -> Result<VaultInfo, String> {
    if !root.is_dir() {
        return Err(format!("{} isn't a folder.", root.display()));
    }
    let _ = app.asset_protocol_scope().allow_directory(root, true);
    *app.state::<Vault>().0.lock().unwrap() = Some(root.to_path_buf());
    let s = root.to_string_lossy().into_owned();
    app.state::<Store>().update(|st| {
        st.vault = Some(s.clone());
        st.recent.retain(|r| r != &s);
        st.recent.insert(0, s.clone());
        st.recent.truncate(8);
    });
    Ok(vault_info(root))
}

#[tauri::command(async)]
fn boot(app: AppHandle, store: State<'_, Store>) -> Result<Boot, String> {
    let s = store.get();
    let saved = s.vault.map(PathBuf::from).filter(|p| p.is_dir());
    let root = match saved {
        Some(p) => p,
        None => {
            let p = default_vault(&app);
            let fresh = !p.exists();
            std::fs::create_dir_all(&p).map_err(|e| format!("Couldn't create {}: {e}", p.display()))?;
            if fresh {
                for (name, text) in welcome::notes() {
                    let _ = vault::write_atomic(&p.join(name), text.as_bytes());
                }
            }
            p
        }
    };
    let info = use_vault(&app, &root)?;
    let s = store.get();
    Ok(Boot {
        vault: info,
        recent: s.recent,
        ui: s.ui,
        platform: PLATFORM,
        voice: cfg!(feature = "voice"),
        version: app.package_info().version.to_string(),
    })
}

#[tauri::command(async)]
fn vault_list(v: State<'_, Vault>) -> Result<Vec<Entry>, String> {
    Ok(vault::list(&v.root()?))
}

#[tauri::command(async)]
fn vault_choose(app: AppHandle) -> Result<Option<VaultInfo>, String> {
    let mut dialog = app.dialog().file().set_title("Choose a folder for your notes");
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    let Some(picked) = dialog.blocking_pick_folder() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    use_vault(&app, &path).map(Some)
}

#[tauri::command(async)]
fn vault_switch(app: AppHandle, path: String) -> Result<VaultInfo, String> {
    use_vault(&app, Path::new(&path))
}

#[tauri::command(async)]
fn vault_forget(store: State<'_, Store>, path: String) -> Vec<String> {
    store.update(|s| {
        s.recent.retain(|r| r != &path);
        s.recent.clone()
    })
}

#[tauri::command(async)]
fn note_read(v: State<'_, Vault>, path: String) -> Result<NoteText, String> {
    vault::read(&v.root()?, &path)
}

#[tauri::command(async)]
fn note_write(v: State<'_, Vault>, path: String, text: String) -> Result<u64, String> {
    vault::write(&v.root()?, &path, &text)
}

#[tauri::command(async)]
fn note_stat(v: State<'_, Vault>, path: String) -> Result<u64, String> {
    Ok(vault::mtime(&vault::resolve(&v.root()?, &path)?))
}

#[tauri::command(async)]
fn note_create(v: State<'_, Vault>, dir: String, name: String, text: String) -> Result<String, String> {
    vault::create_note(&v.root()?, &dir, &name, &text)
}

#[tauri::command(async)]
fn folder_create(v: State<'_, Vault>, dir: String, name: String) -> Result<String, String> {
    vault::create_folder(&v.root()?, &dir, &name)
}

#[tauri::command(async)]
fn entry_rename(v: State<'_, Vault>, from: String, to: String) -> Result<Renamed, String> {
    vault::rename(&v.root()?, &from, &to)
}

#[tauri::command(async)]
fn entry_trash(v: State<'_, Vault>, path: String) -> Result<(), String> {
    vault::trash(&v.root()?, &path)
}

#[tauri::command(async)]
fn vault_search(v: State<'_, Vault>, query: String) -> Result<Vec<Hit>, String> {
    Ok(vault::search(&v.root()?, &query, 120))
}

#[tauri::command(async)]
fn note_backlinks(v: State<'_, Vault>, path: String) -> Result<Vec<Backlink>, String> {
    Ok(vault::backlinks(&v.root()?, &path))
}

/// Raw bytes in the body, the file name in the `x-name` header.
#[tauri::command(async)]
fn attachment_save(v: State<'_, Vault>, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected file bytes.".into());
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|h| h.to_str().ok())
        .map(links::url_decode)
        .unwrap_or_else(|| "Pasted file".into());
    vault::save_attachment(&v.root()?, &name, bytes)
}

#[tauri::command]
fn reveal(app: AppHandle, v: State<'_, Vault>, path: String) -> Result<(), String> {
    let p = vault::resolve(&v.root()?, &path)?;
    app.opener().reveal_item_in_dir(p).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")) {
        return Err("Only web and mail links open outside Pinion.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn settings_ui(app: AppHandle, store: State<'_, Store>, patch: Map<String, Value>) {
    let ui = store.update(|s| {
        settings::merge(&mut s.ui, patch);
        s.ui.clone()
    });
    let _ = app.emit("ui-settings", ui);
}

/// Matches native window chrome and the pre-paint background to the theme.
#[tauri::command]
fn set_appearance(app: AppHandle, store: State<'_, Store>, background: String, dark: bool) {
    store.update(|s| s.background = Some(background.clone()));
    let theme = if dark { tauri::Theme::Dark } else { tauri::Theme::Light };
    if let Some(w) = app.get_webview_window("main") {
        if let Some(c) = parse_hex(&background) {
            let _ = w.set_background_color(Some(c));
        }
        let _ = w.set_theme(Some(theme));
    }
}

#[tauri::command]
fn ui_log(message: String) {
    if cfg!(debug_assertions) {
        eprintln!("[ui] {message}");
    }
}

#[tauri::command]
fn ui_ready(window: WebviewWindow) {
    if !window.is_visible().unwrap_or(true) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn window_action(window: WebviewWindow, action: String) -> Result<bool, String> {
    let r = match action.as_str() {
        "minimize" => window.minimize(),
        "maximize" => {
            if window.is_maximized().unwrap_or(false) {
                window.unmaximize()
            } else {
                window.maximize()
            }
        }
        "close" => window.close(),
        _ => Ok(()),
    };
    r.map_err(|e| e.to_string())?;
    Ok(window.is_maximized().unwrap_or(false))
}

/// Shows the notes window, building it if it was closed.
pub fn show_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    #[cfg(feature = "voice")]
    voice::main_opened(app);
    if let Some(w) = app.get_webview_window("main") {
        w.show()?;
        let _ = w.unminimize();
        w.set_focus()?;
        return Ok(w);
    }
    let s = app.state::<Store>().get();
    let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Pinion")
        .inner_size(1080.0, 740.0)
        .min_inner_size(380.0, 300.0)
        .visible(false)
        .disable_drag_drop_handler();
    if let Some(c) = s.background.as_deref().and_then(parse_hex) {
        b = b.background_color(c);
    }
    match s.window {
        Some(ws) if on_screen(app, &ws) => {
            b = b.inner_size(ws.w, ws.h).position(ws.x, ws.y).maximized(ws.maximized);
        }
        _ => b = b.center(),
    }
    #[cfg(target_os = "macos")]
    {
        b = b
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(LogicalPosition::new(18.0, 22.0));
    }
    #[cfg(not(target_os = "macos"))]
    {
        b = b.decorations(false).shadow(true);
    }
    let w = b.build()?;
    let handle = app.clone();
    w.on_window_event(move |e| match e {
        WindowEvent::CloseRequested { .. } => save_window(&handle),
        WindowEvent::Destroyed => {
            #[cfg(feature = "voice")]
            voice::main_closed(&handle);
        }
        _ => {}
    });
    // If the page never reports in, show the window anyway.
    let late = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(4));
        if !late.is_visible().unwrap_or(true) {
            let _ = late.show();
        }
    });
    Ok(w)
}

fn on_screen(app: &AppHandle, ws: &WindowState) -> bool {
    let Ok(monitors) = app.available_monitors() else { return false };
    monitors.iter().any(|m| {
        let sf = m.scale_factor();
        let p = m.position().to_logical::<f64>(sf);
        let s = m.size().to_logical::<f64>(sf);
        ws.x + 40.0 >= p.x && ws.y >= p.y - 10.0 && ws.x + 40.0 <= p.x + s.width && ws.y + 20.0 <= p.y + s.height
    })
}

fn save_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let Ok(sf) = w.scale_factor() else { return };
    let maximized = w.is_maximized().unwrap_or(false);
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) else { return };
    let pos: LogicalPosition<f64> = pos.to_logical(sf);
    let size: LogicalSize<f64> = size.to_logical(sf);
    app.state::<Store>().update(|s| {
        let keep = s.window.filter(|_| maximized);
        s.window = Some(match keep {
            // Keep the restored size when closing while maximized.
            Some(old) => WindowState { maximized: true, ..old },
            None => WindowState { x: pos.x, y: pos.y, w: size.width, h: size.height, maximized },
        });
    });
}

macro_rules! handlers {
    ($($extra:path),*) => {
        tauri::generate_handler![
            boot, vault_list, vault_choose, vault_switch, vault_forget, note_read, note_write,
            note_stat, note_create, folder_create, entry_rename, entry_trash, vault_search,
            note_backlinks, attachment_save, reveal, open_external, settings_ui, set_appearance,
            ui_ready, ui_log, window_action $(, $extra)*
        ]
    };
}

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            #[cfg(feature = "voice")]
            if voice::handle_args(app, &args) {
                return;
            }
            let _ = args;
            let _ = show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(feature = "voice")]
    let builder = voice::plugins(builder);

    #[cfg(feature = "voice")]
    let builder = builder.invoke_handler(handlers![
        voice::voice_state,
        voice::voice_settings,
        voice::voice_history,
        voice::voice_history_delete,
        voice::voice_history_clear,
        voice::voice_model_download,
        voice::voice_model_cancel,
        voice::voice_model_delete,
        voice::voice_devices,
        voice::voice_toggle,
        voice::voice_cancel,
        voice::meeting_toggle,
        voice::pill_area,
        voice::voice_permissions,
        voice::voice_copy
    ]);
    #[cfg(not(feature = "voice"))]
    let builder = builder.invoke_handler(handlers![]);

    builder
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            app.manage(Store::load(config_dir.join("settings.json")));
            app.manage(Vault(Mutex::new(None)));
            #[cfg(feature = "voice")]
            voice::setup(app.handle())?;
            let args: Vec<String> = std::env::args().collect();
            #[cfg(feature = "voice")]
            if voice::is_background_launch(&args) {
                return Ok(());
            }
            let _ = args;
            show_main(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Pinion couldn't start")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                let _ = show_main(app);
            }
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                // With voice, closing the notes window keeps the pill running.
                if code.is_none() && cfg!(feature = "voice") {
                    api.prevent_exit();
                }
                let _ = app;
            }
            _ => {}
        });
}
