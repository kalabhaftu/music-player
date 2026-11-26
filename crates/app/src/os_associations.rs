//! Native Windows file association and shell verb registration.
//!
//! Replaces external PowerShell scripts with direct Win32 Registry API calls
//! (`RegCreateKeyExW`, `RegSetValueExW`, `RegDeleteTreeW`, `SHChangeNotify`).

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, KEY_SET_VALUE, REG_SZ,
};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    ".mp3", ".flac", ".wav", ".wave", ".aif", ".aiff", ".m4a", ".m4b", ".aac",
    ".ogg", ".oga", ".opus", ".wma", ".ape", ".wv", ".tta", ".mpc", ".dsf", ".dff",
];

const LEGACY_EXTENSIONS: &[&str] = &[".mp4"];

const APP_NAME: &str = "Music Player";
const REG_APP_KEY: &str = "Software\\RegisteredApplications";
const CAPABILITIES_SUBKEY: &str = "Software\\MusicPlayer\\Capabilities\\MusicPlayer";
const INTEGRATION_ROOT: &str = "Software\\MusicPlayer\\ShellIntegration\\MusicPlayer";

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_or_create_key(parent: HKEY, subkey: &str) -> Option<HKEY> {
    let wide_key = to_wide(subkey);
    let mut hkey: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            parent,
            wide_key.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            KEY_ALL_ACCESS,
            std::ptr::null_mut(),
            &mut hkey,
            std::ptr::null_mut(),
        )
    };
    if status == ERROR_SUCCESS {
        Some(hkey)
    } else {
        None
    }
}

fn open_existing_key(parent: HKEY, subkey: &str) -> Option<HKEY> {
    let wide_key = to_wide(subkey);
    let mut hkey: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(parent, wide_key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey)
    };
    if status == ERROR_SUCCESS {
        Some(hkey)
    } else {
        None
    }
}

fn set_reg_sz(hkey: HKEY, value_name: Option<&str>, value: &str) -> bool {
    let wide_val = to_wide(value);
    let wide_name = value_name.map(to_wide);
    let name_ptr = match &wide_name {
        Some(w) => w.as_ptr(),
        None => std::ptr::null(),
    };
    let byte_len = (wide_val.len() * std::mem::size_of::<u16>()) as u32;
    let status = unsafe {
        RegSetValueExW(
            hkey,
            name_ptr,
            0,
            REG_SZ,
            wide_val.as_ptr() as *const u8,
            byte_len,
        )
    };
    status == ERROR_SUCCESS
}

fn set_key_value(subkey: &str, value_name: Option<&str>, value: &str) -> Result<(), String> {
    let hkey = open_or_create_key(HKEY_CURRENT_USER, subkey)
        .ok_or_else(|| format!("Failed to open/create HKCU\\{subkey}"))?;
    let ok = set_reg_sz(hkey, value_name, value);
    unsafe { RegCloseKey(hkey); }
    if ok {
        Ok(())
    } else {
        Err(format!("Failed to write value to HKCU\\{subkey}"))
    }
}

fn delete_tree(subkey: &str) -> bool {
    let wide = to_wide(subkey);
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide.as_ptr()) == ERROR_SUCCESS }
}

fn notify_shell() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
    }
}

/// Registers the application under HKCU for audio file types and context-menu actions.
pub fn register_associations() -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("Cannot locate current executable: {e}"))?;
    let exe_str = exe_path.to_str().ok_or_else(|| "Invalid executable path".to_string())?;
    let exe_name = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("music-player-app.exe");

    let app_key = format!("Software\\Classes\\Applications\\{exe_name}");
    let icon_ref = format!("\"{exe_str}\",0");

    // Integration metadata
    set_key_value(INTEGRATION_ROOT, None, exe_str)?;
    set_key_value(INTEGRATION_ROOT, Some("ExecutablePath"), exe_str)?;
    set_key_value(REG_APP_KEY, Some(APP_NAME), CAPABILITIES_SUBKEY)?;
    set_key_value(CAPABILITIES_SUBKEY, Some("ApplicationName"), APP_NAME)?;
    set_key_value(CAPABILITIES_SUBKEY, Some("ApplicationDescription"), "Offline music library and player")?;
    set_key_value(CAPABILITIES_SUBKEY, Some("ApplicationIcon"), &icon_ref)?;

    // Application shell open & COM DropTarget registration
    let play_clsid = crate::shell_drop::PLAY_CLSID_STR;
    let queue_clsid = crate::shell_drop::QUEUE_CLSID_STR;
    let playlist_clsid = crate::shell_drop::PLAYLIST_CLSID_STR;
    for clsid in [play_clsid, queue_clsid, playlist_clsid] {
        let server_key = format!("Software\\Classes\\CLSID\\{clsid}\\LocalServer32");
        set_key_value(&server_key, None, &format!("\"{exe_str}\" --shell-drop-target"))?;
        set_key_value(&server_key, Some("ServerExecutable"), exe_str)?;
    }

    set_key_value(&format!("{app_key}\\shell\\open\\command"), None, &format!("\"{exe_str}\" \"%1\""))?;
    set_key_value(&format!("{app_key}\\shell\\open"), Some("MultiSelectModel"), "Player")?;
    set_key_value(&format!("{app_key}\\shell\\open\\DropTarget"), Some("CLSID"), play_clsid)?;
    set_key_value(&format!("{app_key}\\DefaultIcon"), None, &icon_ref)?;

    // Clean up legacy .mp4 registration if upgrading from earlier version
    for ext in LEGACY_EXTENSIONS {
        let prog_id = format!("MusicPlayer.AssocFile.{}", ext.trim_start_matches('.'));
        delete_tree(&format!("Software\\Classes\\{prog_id}"));
        delete_tree(&format!("Software\\Classes\\SystemFileAssociations\\{ext}\\shell\\MusicPlayerAddToQueue"));
        delete_tree(&format!("Software\\Classes\\SystemFileAssociations\\{ext}\\shell\\MusicPlayerCreatePlaylist"));
    }

    // Register all supported audio extensions
    for ext in SUPPORTED_EXTENSIONS {
        let ext_clean = ext.trim_start_matches('.');
        let prog_id = format!("MusicPlayer.AssocFile.{ext_clean}");

        // SupportedTypes & Capabilities
        set_key_value(&format!("{app_key}\\SupportedTypes"), Some(ext), "")?;
        set_key_value(&format!("{CAPABILITIES_SUBKEY}\\FileAssociations"), Some(ext), &prog_id)?;

        // OpenWithProgids
        set_key_value(&format!("Software\\Classes\\{ext}\\OpenWithProgids"), Some(&prog_id), "")?;

        // ProgID configuration
        let prog_key = format!("Software\\Classes\\{prog_id}");
        set_key_value(&prog_key, None, &format!("{APP_NAME} audio file"))?;
        set_key_value(&format!("{prog_key}\\DefaultIcon"), None, &icon_ref)?;
        set_key_value(&prog_key, Some("TypeOverlay"), &icon_ref)?;
        set_key_value(&format!("{prog_key}\\shell\\open\\command"), None, &format!("\"{exe_str}\" \"%1\""))?;
        set_key_value(&format!("{prog_key}\\shell\\open"), Some("MultiSelectModel"), "Player")?;
        set_key_value(&format!("{prog_key}\\shell\\open\\DropTarget"), Some("CLSID"), play_clsid)?;

        // Context-menu actions: Add to Queue & Create Playlist (COM DropTarget captures full multi-select)
        let add_queue_key = format!("Software\\Classes\\SystemFileAssociations\\{ext}\\shell\\MusicPlayerAddToQueue");
        set_key_value(&add_queue_key, None, "Add to Music Player queue")?;
        set_key_value(&add_queue_key, Some("Icon"), &icon_ref)?;
        set_key_value(&add_queue_key, Some("MultiSelectModel"), "Player")?;
        set_key_value(&format!("{add_queue_key}\\DropTarget"), Some("CLSID"), queue_clsid)?;
        set_key_value(&format!("{add_queue_key}\\command"), None, &format!("\"{exe_str}\" --add-to-queue \"%1\""))?;

        let playlist_key = format!("Software\\Classes\\SystemFileAssociations\\{ext}\\shell\\MusicPlayerCreatePlaylist");
        set_key_value(&playlist_key, None, "Create Music Player playlist")?;
        set_key_value(&playlist_key, Some("Icon"), &icon_ref)?;
        set_key_value(&playlist_key, Some("MultiSelectModel"), "Player")?;
        set_key_value(&format!("{playlist_key}\\DropTarget"), Some("CLSID"), playlist_clsid)?;
        set_key_value(&format!("{playlist_key}\\command"), None, &format!("\"{exe_str}\" --create-playlist \"%1\""))?;
    }

    notify_shell();
    Ok(())
}

/// Unregisters the application from HKCU file associations and context menu verbs.
pub fn unregister_associations() -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("Cannot locate current executable: {e}"))?;
    let exe_name = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("music-player-app.exe");

    let app_key = format!("Software\\Classes\\Applications\\{exe_name}");
    delete_tree(&app_key);
    delete_tree(CAPABILITIES_SUBKEY);
    delete_tree(INTEGRATION_ROOT);
    for clsid in [
        crate::shell_drop::PLAY_CLSID_STR,
        crate::shell_drop::QUEUE_CLSID_STR,
        crate::shell_drop::PLAYLIST_CLSID_STR,
    ] {
        delete_tree(&format!("Software\\Classes\\CLSID\\{clsid}"));
    }

    // Remove RegisteredApplications value
    if let Some(hkey) = open_existing_key(HKEY_CURRENT_USER, REG_APP_KEY) {
        let wide = to_wide(APP_NAME);
        unsafe {
            RegDeleteValueW(hkey, wide.as_ptr());
            RegCloseKey(hkey);
        }
    }

    for ext in SUPPORTED_EXTENSIONS.iter().chain(LEGACY_EXTENSIONS.iter()) {
        let ext_clean = ext.trim_start_matches('.');
        let prog_id = format!("MusicPlayer.AssocFile.{ext_clean}");

        // Remove from OpenWithProgids
        if let Some(hkey) = open_existing_key(HKEY_CURRENT_USER, &format!("Software\\Classes\\{ext}\\OpenWithProgids")) {
            let wide = to_wide(&prog_id);
            unsafe {
                RegDeleteValueW(hkey, wide.as_ptr());
                RegCloseKey(hkey);
            }
        }

        delete_tree(&format!("Software\\Classes\\{prog_id}"));
        delete_tree(&format!("Software\\Classes\\SystemFileAssociations\\{ext}\\shell\\MusicPlayerAddToQueue"));
        delete_tree(&format!("Software\\Classes\\SystemFileAssociations\\{ext}\\shell\\MusicPlayerCreatePlaylist"));
    }

    notify_shell();
    Ok(())
}
