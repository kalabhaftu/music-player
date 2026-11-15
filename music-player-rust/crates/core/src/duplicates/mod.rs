pub mod fingerprint;
pub mod detector;

pub use fingerprint::{hash_file, get_file_state};
pub use detector::{DuplicateDetector, ExactDuplicateGroup, ExactDuplicateGroupPage};
