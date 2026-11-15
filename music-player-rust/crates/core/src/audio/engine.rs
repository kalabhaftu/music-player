use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

pub struct AudioEngine {
    current_path: Option<String>,
    duration_ms: u64,
    volume: u32, // 0..100
    status: PlaybackStatus,
    vlc: Option<VlcContext>,
}

struct VlcContext {
    _lib: libloading::Library,
    instance: *mut c_void,
    player: *mut c_void,
    current_media: *mut c_void,

    // Function pointers
    fn_release: unsafe extern "C" fn(*mut c_void),
    fn_media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    fn_media_release: unsafe extern "C" fn(*mut c_void),
    fn_player_new_from_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    fn_player_play: unsafe extern "C" fn(*mut c_void) -> i32,
    fn_player_pause: unsafe extern "C" fn(*mut c_void),
    fn_player_stop: unsafe extern "C" fn(*mut c_void),
    fn_player_release: unsafe extern "C" fn(*mut c_void),
    fn_player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    fn_player_set_time: unsafe extern "C" fn(*mut c_void, i64),
    fn_player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    fn_player_is_playing: unsafe extern "C" fn(*mut c_void) -> i32,
    fn_audio_set_volume: unsafe extern "C" fn(*mut c_void, i32) -> i32,
}

impl Drop for VlcContext {
    fn drop(&mut self) {
        unsafe {
            if !self.player.is_null() {
                (self.fn_player_stop)(self.player);
                (self.fn_player_release)(self.player);
            }
            if !self.current_media.is_null() {
                (self.fn_media_release)(self.current_media);
            }
            if !self.instance.is_null() {
                (self.fn_release)(self.instance);
            }
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        let vlc = Self::try_init_vlc();
        Self {
            current_path: None,
            duration_ms: 0,
            volume: 75,
            status: PlaybackStatus::Stopped,
            vlc,
        }
    }

    fn try_init_vlc() -> Option<VlcContext> {
        let possible_paths = [
            PathBuf::from("libvlc.dll"),
            PathBuf::from("artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"),
            PathBuf::from("../artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"),
            PathBuf::from("../../artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"),
        ];

        let mut dll_path = None;
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let candidate = parent.join("libvlc.dll");
                if candidate.exists() {
                    dll_path = Some(candidate);
                }
            }
        }

        if dll_path.is_none() {
            for p in &possible_paths {
                if p.exists() {
                    dll_path = Some(p.clone());
                    break;
                }
            }
        }

        let dll_path = dll_path?;

        // Set VLC_PLUGIN_PATH if plugins directory exists next to dll
        if let Some(parent) = dll_path.parent() {
            let plugins_dir = parent.join("plugins");
            if plugins_dir.exists() {
                std::env::set_var("VLC_PLUGIN_PATH", plugins_dir);
            }
        }

        unsafe {
            let lib = libloading::Library::new(&dll_path).ok()?;
            let fn_new: unsafe extern "C" fn(i32, *const *const c_char) -> *mut c_void =
                *lib.get(b"libvlc_new").ok()?;
            let fn_release: unsafe extern "C" fn(*mut c_void) =
                *lib.get(b"libvlc_release").ok()?;
            let fn_media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
                *lib.get(b"libvlc_media_new_path").ok()?;
            let fn_media_release: unsafe extern "C" fn(*mut c_void) =
                *lib.get(b"libvlc_media_release").ok()?;
            let fn_player_new_from_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
                *lib.get(b"libvlc_media_player_new_from_media").ok()?;
            let fn_player_play: unsafe extern "C" fn(*mut c_void) -> i32 =
                *lib.get(b"libvlc_media_player_play").ok()?;
            let fn_player_pause: unsafe extern "C" fn(*mut c_void) =
                *lib.get(b"libvlc_media_player_pause").ok()?;
            let fn_player_stop: unsafe extern "C" fn(*mut c_void) =
                *lib.get(b"libvlc_media_player_stop").ok()?;
            let fn_player_release: unsafe extern "C" fn(*mut c_void) =
                *lib.get(b"libvlc_media_player_release").ok()?;
            let fn_player_get_time: unsafe extern "C" fn(*mut c_void) -> i64 =
                *lib.get(b"libvlc_media_player_get_time").ok()?;
            let fn_player_set_time: unsafe extern "C" fn(*mut c_void, i64) =
                *lib.get(b"libvlc_media_player_set_time").ok()?;
            let fn_player_get_length: unsafe extern "C" fn(*mut c_void) -> i64 =
                *lib.get(b"libvlc_media_player_get_length").ok()?;
            let fn_player_is_playing: unsafe extern "C" fn(*mut c_void) -> i32 =
                *lib.get(b"libvlc_media_player_is_playing").ok()?;
            let fn_audio_set_volume: unsafe extern "C" fn(*mut c_void, i32) -> i32 =
                *lib.get(b"libvlc_audio_set_volume").ok()?;

            let instance = fn_new(0, std::ptr::null());
            if instance.is_null() {
                return None;
            }

            Some(VlcContext {
                _lib: lib,
                instance,
                player: std::ptr::null_mut(),
                current_media: std::ptr::null_mut(),
                fn_release,
                fn_media_new_path,
                fn_media_release,
                fn_player_new_from_media,
                fn_player_play,
                fn_player_pause,
                fn_player_stop,
                fn_player_release,
                fn_player_get_time,
                fn_player_set_time,
                fn_player_get_length,
                fn_player_is_playing,
                fn_audio_set_volume,
            })
        }
    }

    pub fn play_track<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        if let Some(ref mut vlc) = self.vlc {
            unsafe {
                if !vlc.player.is_null() {
                    (vlc.fn_player_stop)(vlc.player);
                    (vlc.fn_player_release)(vlc.player);
                    vlc.player = std::ptr::null_mut();
                }
                if !vlc.current_media.is_null() {
                    (vlc.fn_media_release)(vlc.current_media);
                    vlc.current_media = std::ptr::null_mut();
                }

                let c_path = CString::new(path_str.as_bytes())
                    .map_err(|e| format!("Invalid path: {}", e))?;
                let media = (vlc.fn_media_new_path)(vlc.instance, c_path.as_ptr());
                if media.is_null() {
                    return Err("Failed to create LibVLC media".into());
                }
                vlc.current_media = media;

                let player = (vlc.fn_player_new_from_media)(media);
                if player.is_null() {
                    return Err("Failed to create LibVLC media player".into());
                }
                vlc.player = player;

                (vlc.fn_audio_set_volume)(player, self.volume as i32);
                (vlc.fn_player_play)(player);
            }

            self.current_path = Some(path_str);
            self.status = PlaybackStatus::Playing;
            return Ok(());
        }

        // Fallback to MCI on Windows if LibVLC is not available
        #[cfg(windows)]
        {
            let _ = Self::send_mci("close mp_audio");
            let open_cmd = format!("open \"{}\" type mpegvideo alias mp_audio", path_str);
            if let Err(e) = Self::send_mci(&open_cmd) {
                let fallback_cmd = format!("open \"{}\" alias mp_audio", path_str);
                Self::send_mci(&fallback_cmd).map_err(|e2| format!("{}: {}", e, e2))?;
            }
            let _ = Self::send_mci("set mp_audio time format milliseconds");
            if let Ok(len_str) = Self::send_mci("status mp_audio length") {
                if let Ok(len) = len_str.trim().parse::<u64>() {
                    self.duration_ms = len;
                }
            }
            let _ = Self::send_mci("play mp_audio");
            let _ = self.set_volume(self.volume);
            self.current_path = Some(path_str);
            self.status = PlaybackStatus::Playing;
            Ok(())
        }

        #[cfg(not(windows))]
        {
            self.current_path = Some(path_str);
            self.status = PlaybackStatus::Playing;
            Ok(())
        }
    }

    pub fn pause(&mut self) -> Result<(), String> {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                unsafe {
                    (vlc.fn_player_pause)(vlc.player);
                }
                self.status = PlaybackStatus::Paused;
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            Self::send_mci("pause mp_audio")?;
        }
        self.status = PlaybackStatus::Paused;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                unsafe {
                    (vlc.fn_player_play)(vlc.player);
                }
                self.status = PlaybackStatus::Playing;
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            Self::send_mci("resume mp_audio")?;
        }
        self.status = PlaybackStatus::Playing;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                unsafe {
                    (vlc.fn_player_stop)(vlc.player);
                }
                self.status = PlaybackStatus::Stopped;
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            Self::send_mci("stop mp_audio")?;
        }
        self.status = PlaybackStatus::Stopped;
        Ok(())
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                unsafe {
                    (vlc.fn_player_set_time)(vlc.player, position_ms as i64);
                }
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            let cmd = format!("seek mp_audio to {}", position_ms);
            Self::send_mci(&cmd)?;
            if self.status == PlaybackStatus::Playing {
                Self::send_mci("play mp_audio")?;
            }
        }
        Ok(())
    }

    pub fn set_volume(&mut self, volume: u32) -> Result<(), String> {
        self.volume = volume.min(100);

        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                unsafe {
                    (vlc.fn_audio_set_volume)(vlc.player, self.volume as i32);
                }
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            let mci_vol = (self.volume as f64 * 10.0) as u32; // 0..1000
            let cmd = format!("setaudio mp_audio volume to {}", mci_vol);
            let _ = Self::send_mci(&cmd);
        }
        Ok(())
    }

    pub fn position_ms(&self) -> u64 {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                let pos = unsafe { (vlc.fn_player_get_time)(vlc.player) };
                if pos >= 0 {
                    return pos as u64;
                }
            }
        }

        #[cfg(windows)]
        {
            if let Ok(pos_str) = Self::send_mci("status mp_audio position") {
                if let Ok(pos) = pos_str.trim().parse::<u64>() {
                    return pos;
                }
            }
        }
        0
    }

    pub fn duration_ms(&self) -> u64 {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                let len = unsafe { (vlc.fn_player_get_length)(vlc.player) };
                if len > 0 {
                    return len as u64;
                }
            }
        }
        self.duration_ms
    }

    pub fn is_playing(&self) -> bool {
        if let Some(ref vlc) = self.vlc {
            if !vlc.player.is_null() {
                let playing = unsafe { (vlc.fn_player_is_playing)(vlc.player) };
                return playing != 0;
            }
        }
        self.status == PlaybackStatus::Playing
    }

    #[cfg(windows)]
    fn send_mci(command: &str) -> Result<String, String> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "winmm")]
        extern "system" {
            fn mciSendStringW(
                lpstr_command: *const u16,
                lpstr_return_string: *mut u16,
                u_return_length: u32,
                hwnd_callback: usize,
            ) -> u32;
        }

        let mut wide: Vec<u16> = OsStr::new(command).encode_wide().collect();
        wide.push(0);

        let mut return_buf = [0u16; 256];
        let err = unsafe {
            mciSendStringW(
                wide.as_ptr(),
                return_buf.as_mut_ptr(),
                return_buf.len() as u32,
                0,
            )
        };

        if err != 0 {
            return Err(format!("MCI error code: {}", err));
        }

        let len = return_buf.iter().position(|&c| c == 0).unwrap_or(return_buf.len());
        let res = String::from_utf16_lossy(&return_buf[..len]);
        Ok(res)
    }
}
