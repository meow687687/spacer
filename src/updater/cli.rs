use anyhow::Result;
use crate::updater::self_update::execute_self_update;
use crate::updater::version::{check_for_updates, get_current_version};

pub fn run_cli_update() -> Result<()> {
    let current_ver = get_current_version();
    
    println!("\x1b[1;36m⚡ Spacer Updater\x1b[0m");
    println!("  \x1b[96m[1/3]\x1b[0m Checking GitHub Releases for updates... (current: \x1b[1mv{}\x1b[0m)", current_ver);

    match check_for_updates(true)? {
        Some(info) => {
            println!("  \x1b[92m  ✓\x1b[0m Found new release: \x1b[1;92m{}\x1b[0m (\x1b[2mv{} ➔ {}\x1b[0m)", info.latest_version, current_ver, info.latest_version);
            
            if !info.release_notes.trim().is_empty() {
                println!("\n  \x1b[1;33mRelease Notes:\x1b[0m");
                for line in info.release_notes.trim().lines().take(8) {
                    println!("    \x1b[2m│\x1b[0m {}", line);
                }
                println!();
            }

            println!("  \x1b[96m[2/3]\x1b[0m Downloading release archive for host platform...");
            println!("  \x1b[96m[3/3]\x1b[0m Applying atomic in-place binary update...");

            match execute_self_update(&info) {
                Ok(_) => {
                    println!("  \x1b[92m  ✓\x1b[0m Binary replaced atomically");
                    println!("\n  ┌────────────────────────────────────────────────────────┐");
                    println!("  │  \x1b[92m✓ Update Successful!\x1b[0m                                  │");
                    println!("  │  Spacer has been updated to \x1b[1;96m{:<15}\x1b[0m            │", info.latest_version);
                    println!("  │  Run \x1b[32mspacer --version\x1b[0m or \x1b[32mspacer\x1b[0m to enjoy the new build! │");
                    println!("  └────────────────────────────────────────────────────────┘\n");
                }
                Err(e) => {
                    eprintln!("\n  \x1b[33m! In-place self-update encountered an issue:\x1b[0m {}", e);
                    println!("  \x1b[2mFalling back to automated install script...\x1b[0m");
                    let status = std::process::Command::new("bash")
                        .arg("-c")
                        .arg("curl -fsSL https://raw.githubusercontent.com/meow687687/spacer/main/install.sh | bash")
                        .status();
                    if let Ok(s) = status {
                        if s.success() {
                            println!("  \x1b[92m✓\x1b[0m Spacer updated successfully via installer script!");
                            return Ok(());
                        }
                    }
                }
            }
        }
        None => {
            println!("  \x1b[92m  ✓\x1b[0m You are already on the latest version of Spacer (\x1b[1;92mv{}\x1b[0m)!", current_ver);
        }
    }

    Ok(())
}
