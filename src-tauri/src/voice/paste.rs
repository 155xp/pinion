//! Puts words where the cursor is: through the clipboard and a paste keystroke,
//! then puts the clipboard back the way it was.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::sync::Mutex;
use std::time::Duration;

// One clipboard for the life of the app; on Linux the owner must stay alive.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

pub enum Pasted {
    Typed,
    /// Left on the clipboard because the system wouldn't let us press keys.
    Copied,
}

pub fn paste(text: &str) -> Result<Pasted, String> {
    let previous = {
        let mut guard = CLIPBOARD.lock().unwrap();
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(|e| format!("No clipboard: {e}"))?);
        }
        let cb = guard.as_mut().unwrap();
        let previous = cb.get_text().ok();
        cb.set_text(text.to_string()).map_err(|e| format!("Couldn't use the clipboard: {e}"))?;
        previous
    };
    std::thread::sleep(Duration::from_millis(40));

    if !super::platform::can_send_keys() {
        return Ok(Pasted::Copied);
    }
    if !send_paste() {
        return Ok(Pasted::Copied);
    }

    // Give the app time to read the clipboard before restoring it.
    let ours = text.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        let mut guard = CLIPBOARD.lock().unwrap();
        if let (Some(cb), Some(prev)) = (guard.as_mut(), previous) {
            if cb.get_text().ok().as_deref() == Some(ours.as_str()) {
                let _ = cb.set_text(prev);
            }
        }
    });
    Ok(Pasted::Typed)
}

fn send_paste() -> bool {
    #[cfg(target_os = "linux")]
    if std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false) {
        // On Wayland only tools the compositor trusts can press keys.
        for (cmd, args) in [("wtype", &["-M", "ctrl", "v", "-m", "ctrl"][..]), ("ydotool", &["key", "29:1", "47:1", "47:0", "29:0"][..])] {
            if std::process::Command::new(cmd).args(args).status().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else { return false };
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    let ok = enigo.key(modifier, Direction::Press).is_ok()
        && enigo.key(Key::Unicode('v'), Direction::Click).is_ok();
    let _ = enigo.key(modifier, Direction::Release);
    ok
}

pub fn copy(text: &str) {
    let mut guard = CLIPBOARD.lock().unwrap();
    if guard.is_none() {
        *guard = arboard::Clipboard::new().ok();
    }
    if let Some(cb) = guard.as_mut() {
        let _ = cb.set_text(text.to_string());
    }
}
