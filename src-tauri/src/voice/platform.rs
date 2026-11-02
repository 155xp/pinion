//! The few things that differ by operating system.

#[cfg(target_os = "macos")]
mod mac {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn ask() -> bool {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
        unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const c_void) }
    }
}

/// Whether Pinion may press keys in other apps. Only macOS asks.
pub fn can_send_keys() -> bool {
    #[cfg(target_os = "macos")]
    return mac::trusted();
    #[cfg(not(target_os = "macos"))]
    true
}

/// `None` where no permission is needed.
pub fn accessibility(request: bool) -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        if request {
            let ok = mac::ask();
            if !ok {
                let _ = std::process::Command::new("open")
                    .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                    .spawn();
            }
            return Some(ok);
        }
        return Some(mac::trusted());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = request;
        None
    }
}

/// On macOS, hide the Dock icon while only the pill is showing.
pub fn dock(app: &tauri::AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if visible { tauri::ActivationPolicy::Regular } else { tauri::ActivationPolicy::Accessory };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);
}

/// A key label in the platform's own words: "⌥ Space" or "Ctrl Shift Space".
pub fn shortcut_label(s: &str) -> String {
    s.split('+')
        .map(|k| {
            let k = k.trim();
            let mac = cfg!(target_os = "macos");
            match k.to_ascii_lowercase().as_str() {
                "alt" | "option" => if mac { "⌥" } else { "Alt" }.to_string(),
                "ctrl" | "control" => if mac { "⌃" } else { "Ctrl" }.to_string(),
                "shift" => if mac { "⇧" } else { "Shift" }.to_string(),
                "super" | "cmd" | "command" | "meta" => if mac { "⌘" } else { "Win" }.to_string(),
                "cmdorctrl" | "commandorcontrol" => if mac { "⌘" } else { "Ctrl" }.to_string(),
                _ => k.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
