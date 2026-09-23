//! Raw-byte SHA-256 for the release manifest, checked when a release is downloaded.
use sha2::{Digest, Sha256};

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
