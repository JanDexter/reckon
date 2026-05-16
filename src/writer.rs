//! Atomic file writer used by the Memoir Engine.
//!
//! Writes to a `.tmp.<n>` sibling and renames over the destination so a
//! crashed or aborted write never leaves a corrupt `MEMOIR.md` on disk.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::Result;

pub fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension({
        let mut s = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !s.is_empty() {
            s.push('.');
        }
        s.push_str("reckon.tmp");
        s
    });
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}
