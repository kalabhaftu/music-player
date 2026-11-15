use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn mciSendStringW(
        lpstr_command: *const u16,
        lpstr_return_string: *mut u16,
        u_return_length: u32,
        hwnd_callback: usize,
    ) -> u32;
}

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
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            current_path: None,
            duration_ms: 0,
            volume: 75,
            status: PlaybackStatus::Stopped,
        }
    }

    #[cfg(windows)]
    fn send_mci(command: &str) -> Result<String, String> {
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

    #[cfg(not(windows))]
    fn send_mci(_command: &str) -> Result<String, String> {
        Ok(String::new())
    }

    pub fn play_track<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        #[cfg(windows)]
        {
            // Close any previously opened audio device
            let _ = Self::send_mci("close mp_audio");

            // Open new track with alias 'mp_audio'
            let open_cmd = format!("open \"{}\" type mpegvideo alias mp_audio", path_str);
            if let Err(e) = Self::send_mci(&open_cmd) {
                // Try opening without type flag for wav/other formats
                let fallback_cmd = format!("open \"{}\" alias mp_audio", path_str);
                Self::send_mci(&fallback_cmd).map_err(|e2| format!("{}: {}", e, e2))?;
            }

            // Set time format to milliseconds
            let _ = Self::send_mci("set mp_audio time format milliseconds");

            // Query duration
            if let Ok(len_str) = Self::send_mci("status mp_audio length") {
                if let Ok(len) = len_str.trim().parse::<u64>() {
                    self.duration_ms = len;
                }
            }

            // Set initial volume (MCI uses 0..1000)
            let mci_vol = (self.volume.min(100) * 10).min(1000);
            let _ = Self::send_mci(&format!("setaudio mp_audio volume to {}", mci_vol));

            // Start playing
            Self::send_mci("play mp_audio")?;
        }

        self.current_path = Some(path_str);
        self.status = PlaybackStatus::Playing;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        {
            Self::send_mci("pause mp_audio")?;
        }
        self.status = PlaybackStatus::Paused;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        {
            Self::send_mci("resume mp_audio")?;
        }
        self.status = PlaybackStatus::Playing;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        {
            let _ = Self::send_mci("stop mp_audio");
            let _ = Self::send_mci("close mp_audio");
        }
        self.status = PlaybackStatus::Stopped;
        Ok(())
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        #[cfg(windows)]
        {
            let cmd = format!("seek mp_audio to {}", position_ms);
            Self::send_mci(&cmd)?;
            if self.status == PlaybackStatus::Playing {
                let _ = Self::send_mci("play mp_audio");
            }
        }
        Ok(())
    }

    pub fn set_volume(&mut self, volume: u32) -> Result<(), String> {
        self.volume = volume.min(100);
        #[cfg(windows)]
        {
            let mci_vol = (self.volume * 10).min(1000);
            let _ = Self::send_mci(&format!("setaudio mp_audio volume to {}", mci_vol));
        }
        Ok(())
    }

    pub fn position_ms(&self) -> u64 {
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
        self.duration_ms
    }

    pub fn is_playing(&self) -> bool {
        #[cfg(windows)]
        {
            if let Ok(mode) = Self::send_mci("status mp_audio mode") {
                return mode.trim().eq_ignore_ascii_case("playing");
            }
        }
        self.status == PlaybackStatus::Playing
    }

    pub fn status(&self) -> PlaybackStatus {
        self.status
    }

    pub fn current_path(&self) -> Option<&str> {
        self.current_path.as_deref()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
