//! Cross-platform browser opener for user-facing web surfaces.

use std::process::Command;

use crate::commands::CliError;

pub fn open_url(url: &str) -> Result<(), CliError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CliError::Usage("browser url must not be empty".into()));
    }

    let status = open_url_impl(url).map_err(CliError::Usage)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "browser opener exited with status {}",
            status
        )))
    }
}

fn open_url_impl(url: &str) -> Result<std::process::ExitStatus, String> {
    #[cfg(target_os = "windows")]
    {
        [
            Command::new("rundll32")
                .args(["url.dll,FileProtocolHandler", url])
                .status(),
            Command::new("cmd").args(["/C", "start", "", url]).status(),
        ]
        .into_iter()
        .flatten()
        .next()
        .ok_or_else(|| "failed to open browser on Windows".into())
    }

    #[cfg(target_os = "macos")]
    {
        return Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| e.to_string());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for candidate in ["xdg-open", "gio", "gnome-open", "sensible-browser"] {
            let attempt = if candidate == "gio" {
                Command::new(candidate).args(["open", url]).status()
            } else {
                Command::new(candidate).arg(url).status()
            };
            if let Ok(status) = attempt {
                return Ok(status);
            }
        }
        Err("failed to open browser with xdg-open/gio/gnome-open".into())
    }
}
