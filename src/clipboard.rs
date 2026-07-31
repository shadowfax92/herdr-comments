use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::format::normalize_source;

pub fn read_macos() -> Result<String> {
    acquire_with(|| {
        let output = Command::new("/usr/bin/pbpaste")
            .output()
            .context("failed to run pbpaste")?;
        if !output.status.success() {
            bail!("pbpaste could not read the text clipboard");
        }
        Ok(output.stdout)
    })
}

pub fn acquire_with(read: impl FnOnce() -> Result<Vec<u8>>) -> Result<String> {
    let bytes = read()?;
    let text = String::from_utf8(bytes).context("the clipboard does not contain UTF-8 text")?;
    normalize_source(&text)
}
