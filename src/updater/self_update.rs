use std::fs::{self, File};
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;

use crate::updater::version::UpdateInfo;

pub fn execute_self_update(info: &UpdateInfo) -> Result<()> {
    if let Some(ref download_url) = info.download_url {
        let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
        let temp_file_path = temp_dir.path().join("spacer_download.tmp");

        // Download asset
        let resp = ureq::get(download_url)
            .set("User-Agent", "spacer-updater")
            .call()
            .context("Failed to download release asset from GitHub")?;

        let mut reader = resp.into_reader();
        let mut file = File::create(&temp_file_path).context("Failed to create temporary file")?;
        std::io::copy(&mut reader, &mut file).context("Failed to write downloaded bytes")?;

        let extracted_binary_path = temp_dir.path().join("spacer_extracted");

        // Check if archive or raw binary
        if download_url.ends_with(".tar.gz") || download_url.ends_with(".tgz") {
            let tar_gz = File::open(&temp_file_path)?;
            let tar = GzDecoder::new(tar_gz);
            let mut archive = Archive::new(tar);

            let mut found = false;
            for entry_res in archive.entries()? {
                let mut entry = entry_res?;
                let path = entry.path()?;
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if file_name == "spacer" || file_name == "spacer.exe" {
                    entry.unpack(&extracted_binary_path)?;
                    found = true;
                    break;
                }
            }

            if !found {
                anyhow::bail!("Could not find 'spacer' executable inside downloaded tarball");
            }
        } else {
            // Raw binary
            fs::copy(&temp_file_path, &extracted_binary_path)?;
        }

        // Set executable permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&extracted_binary_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&extracted_binary_path, perms)?;
        }

        // Perform atomic in-place binary self-replacement
        self_replace::self_replace(&extracted_binary_path)
            .context("Failed to replace running executable in-place")?;

        Ok(())
    } else {
        anyhow::bail!("No download URL available for current platform. Please run: curl -fsSL https://raw.githubusercontent.com/meow687687/spacer/main/install.sh | bash")
    }
}
