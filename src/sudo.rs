//! One sudo prompt for the whole run, ported from osx-baseline's Setup.sh:
//! a cached timestamp alone is not enough because every `brew` invocation
//! runs `sudo --reset-timestamp`. Instead the password is stashed in the
//! macOS login Keychain and exposed to every child process via SUDO_ASKPASS
//! (Homebrew adds `-A` to its internal sudo calls when it's set; scripts use
//! `sudo -A`). The guard cleans up on drop: Keychain item deleted, askpass
//! removed, sudo timestamp dropped.

use std::process::Command;

use anyhow::{bail, Context, Result};

const SERVICE: &str = "kitout-sudo";

pub struct SudoGuard {
    dir: Option<tempfile::TempDir>,
}

impl Drop for SudoGuard {
    fn drop(&mut self) {
        if self.dir.take().is_some() {
            let user = std::env::var("USER").unwrap_or_default();
            let _ = Command::new("security")
                .args(["delete-generic-password", "-a", &user, "-s", SERVICE])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            let _ = Command::new("/usr/bin/sudo").arg("-K").status();
            std::env::remove_var("SUDO_ASKPASS");
        }
    }
}

/// Interactive: prompt once (up to 3 attempts), stash, export SUDO_ASKPASS.
/// Non-interactive: no prompt — warn and continue (steps that truly need sudo
/// will surface it themselves).
pub fn setup(interactive: bool) -> Result<SudoGuard> {
    if !interactive {
        crate::ui::warn("sudo: non-interactive run — privileged steps may prompt or fail");
        return Ok(SudoGuard { dir: None });
    }
    let user = std::env::var("USER").context("USER not set")?;
    let dir = tempfile::TempDir::new()?;
    let askpass = dir.path().join("askpass");
    std::fs::write(
        &askpass,
        format!("#!/bin/sh\nexec security find-generic-password -a \"{user}\" -s {SERVICE} -w\n"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700))?;
    }
    std::env::set_var("SUDO_ASKPASS", &askpass);

    for attempt in 1..=3 {
        let pw = crate::ui::sync(|| {
            dialoguer::Password::new()
                .with_prompt("sudo password (asked once for the whole run)")
                .interact()
        })?;
        let stored = Command::new("security")
            .args([
                "add-generic-password",
                "-a",
                &user,
                "-s",
                SERVICE,
                "-U",
                "-w",
                &pw,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("storing sudo password in Keychain")?;
        if !stored.success() {
            bail!("Keychain write failed");
        }
        // -k: ignore any cached timestamp so the password is actually verified
        let ok = Command::new("sudo")
            .args(["-kA", "-v"])
            .stderr(std::process::Stdio::null())
            .status()?
            .success();
        if ok {
            return Ok(SudoGuard { dir: Some(dir) });
        }
        crate::ui::warn(&format!("sorry, try again ({attempt}/3)"));
    }
    bail!("could not validate sudo credentials")
}
