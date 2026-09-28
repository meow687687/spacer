use anyhow::Result;
use crate::updater::self_update::execute_self_update;
use crate::updater::version::{check_for_updates, get_current_version};

pub fn run_cli_update() -> Result<()> {
    println!("⚡ Checking for Spacer updates...");
    let current_ver = get_current_version();
    println!("Current version: v{}", current_ver);

    match check_for_updates(true)? {
        Some(info) => {
            println!("Found new version: {}!", info.latest_version);
            println!("\nRelease Notes:\n{}\n", info.release_notes.trim());
            println!("Downloading and updating Spacer in-place...");

            match execute_self_update(&info) {
                Ok(_) => {
                    println!("\n✓ Spacer was successfully updated to {}!", info.latest_version);
                    println!("Run 'spacer --version' to verify.");
                }
                Err(e) => {
                    eprintln!("\n⚠️ In-place update failed: {}", e);
                    println!("Falling back to installation script...");
                    let status = std::process::Command::new("bash")
                        .arg("-c")
                        .arg("curl -fsSL https://raw.githubusercontent.com/meow687687/spacer/main/install.sh | bash")
                        .status();
                    if let Ok(s) = status {
                        if s.success() {
                            println!("✓ Spacer updated successfully via install script!");
                            return Ok(());
                        }
                    }
                }
            }
        }
        None => {
            println!("✓ Spacer is already up to date (v{})!", current_ver);
        }
    }

    Ok(())
}
