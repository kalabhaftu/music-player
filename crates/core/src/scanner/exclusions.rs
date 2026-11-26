use std::path::Path;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "wave", "aif", "aiff", "m4a", "m4b", "aac", "ogg", "oga", "opus",
    "wma", "ape", "wv", "tta", "mpc", "dsf", "dff",
];

pub const SYSTEM_FOLDERS: &[&str] = &[
    "windows",
    "windows.old",
    "program files",
    "program files (x86)",
    "programdata",
    "windowsapps",
    "recovery",
    "system volume information",
    "$recycle.bin",
    "perflogs",
];

pub const EXCLUDED_FOLDERS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".vs",
    ".idea",
    ".vscode",
    "node_modules",
    "bin",
    "obj",
    "build",
    "dist",
    "target",
    "packages",
    "testresults",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".tox",
    ".venv",
    ".cache",
    ".npm",
    ".bun",
    ".cargo",
    ".rustup",
    ".pnpm",
    ".yarn",
    ".docker",
    ".conda",
    ".gemini",
    ".antigravity",
    ".local",
    ".config",
    ".dotnet",
    ".nuget",
    ".gradle",
    ".obsidian",
    ".trash",
    "temp",
    "tmp",
    "caches",
    "crashdumps",
];

pub fn is_supported_audio<P: AsRef<Path>>(path: P) -> bool {
    if let Some(ext) = path.as_ref().extension().and_then(|s| s.to_str()) {
        let lower = ext.to_ascii_lowercase();
        AUDIO_EXTENSIONS.iter().any(|&e| e == lower)
    } else {
        false
    }
}

pub fn is_excluded_directory(dir_name: &str) -> bool {
    let lower = dir_name.to_ascii_lowercase();
    SYSTEM_FOLDERS.iter().any(|&s| s == lower) || EXCLUDED_FOLDERS.iter().any(|&e| e == lower)
}
