use anyhow::{bail, Context};
use crate::{
    Result,
    util::rand::rand_hex,
};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    keep: bool,
    cleaned_up: bool,
}

impl TempDir {
    pub fn create(out_dir_path: &Path, keep: bool) -> Result<TempDir> {
        let temp_path = out_dir_path.join(
            format!("temp/{time}_{pid}_{rand}",
                    time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs,
                                                              true /* use_z */),
                    pid = std::process::id(),
                    rand = rand_hex(8)));

        tracing::debug!(temp_path = %temp_path.display(),
                        keep,
                        "TempDir::create");

        std::fs::create_dir_all(&*temp_path)?;

        Ok(TempDir {
            path: temp_path,
            keep: keep,
            cleaned_up: false,
        })
    }

    pub fn path(&self) -> Result<&Path> {
        if self.cleaned_up {
            bail!("TempDir already cleaned up.")
        } else {
            Ok(&*self.path)
        }
    }

    pub fn cleanup(&mut self) -> Result<()> {
        tracing::debug!(path = %self.path.display(),
                        cleaned_up = self.cleaned_up,
                        keep = self.keep,
                        "TempDir::cleanup");

        if self.cleaned_up {
            return Ok(());
        }
        // Set self.cleaned_up = true whether or not the delete succeeds.
        self.cleaned_up = true;

        if !self.path.try_exists()? {
            return Ok(());
        }

        if !self.keep {
            std::fs::remove_dir_all(&*self.path)
                .with_context(|| format!("while cleaning up TempDir path='{path}'",
                                         path = self.path.display()))?;
        }
        Ok(())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(err) = self.cleanup() {
            tracing::error!(%err, "TempDir::drop error from cleanup");
        }
    }
}
