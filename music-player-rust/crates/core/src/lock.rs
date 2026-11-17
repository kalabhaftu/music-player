//! Cross-process file write lock — port of `FilePathLock.cs`.
//!
//! A global named mutex keyed by the SHA-256 of the canonicalized path;
//! abandoned locks (previous writer exited) are tolerated.

use std::path::Path;
use sha2::{Digest, Sha256};

pub struct FilePathLock {
    handle: Option<windows_raw::MutexGuard>,
    #[allow(dead_code)]
    name: String,
}

#[cfg(windows)]
mod windows_raw {
    pub struct MutexGuard {
        handle: isize,
        owned: bool,
    }

    impl MutexGuard {
        pub fn acquire(name: &str) -> Option<MutexGuard> {
            use windows_sys::Win32::Foundation::{CloseHandle, WAIT_ABANDONED};
            use windows_sys::Win32::System::Threading::{
                CreateMutexW, WaitForSingleObject, INFINITE,
            };

            let mut wide: Vec<u16> = name.encode_utf16().collect();
            wide.push(0);
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
            if handle.is_null() {
                return None;
            }
            let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
            if wait != 0 && wait != WAIT_ABANDONED {
                unsafe {
                    CloseHandle(handle);
                }
                return None;
            }
            Some(MutexGuard { handle: handle as isize, owned: true })
        }

        pub fn abandoned_takeover(name: &str) -> Option<MutexGuard> {
            Self::acquire(name)
        }
    }

    impl Drop for MutexGuard {
        fn drop(&mut self) {
            if self.owned {
                unsafe {
                    use windows_sys::Win32::Foundation::CloseHandle;
                    use windows_sys::Win32::System::Threading::ReleaseMutex;
                    ReleaseMutex(self.handle as *mut _);
                    CloseHandle(self.handle as *mut _);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod windows_raw {
    pub struct MutexGuard;
    impl MutexGuard {
        pub fn acquire(_name: &str) -> Option<MutexGuard> {
            Some(MutexGuard)
        }
        pub fn abandoned_takeover(name: &str) -> Option<MutexGuard> {
            Self::acquire(name)
        }
    }
}

impl FilePathLock {
    pub fn acquire(path: &Path) -> FilePathLock {
        let name = lock_name(path);
        let handle = windows_raw::MutexGuard::acquire(&name)
            .or_else(|| windows_raw::MutexGuard::abandoned_takeover(&name));
        FilePathLock { handle, name }
    }

    pub fn is_held(&self) -> bool {
        self.handle.is_some()
    }
}

fn lock_name(path: &Path) -> String {
    let canonical = path.to_string_lossy().to_uppercase();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect();
    format!("MusicPlayer.FileWrite.{}", digest)
}
