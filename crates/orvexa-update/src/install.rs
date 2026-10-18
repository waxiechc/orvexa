//! Replacing the running app with a verified download.
//!
//! The new build is unpacked next to the current one, checked, and swapped
//! in by renames on the same volume. The previous build is kept as
//! `ORVEXA.app.previous` / `ORVEXA.exe.previous` until the new one has
//! started successfully ([`finish`]), so a failed update can be undone by
//! renaming it back. Nothing here runs unless the file's SHA-256 matched the
//! signed manifest.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallError(pub String);

fn err(message: impl Into<String>) -> InstallError {
    InstallError(message.into())
}

/// Location of the installed app: the `.app` bundle on macOS, the `.exe` on Windows.
pub fn installed_path() -> Result<PathBuf, InstallError> {
    let exe = std::env::current_exe().map_err(|e| err(e.to_string()))?;
    if cfg!(target_os = "macos") {
        // …/ORVEXA.app/Contents/MacOS/ORVEXA
        let app = exe
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or_else(|| err("not inside an app bundle"))?;
        if app.extension().and_then(|e| e.to_str()) != Some("app") {
            return Err(err(
                "not running from ORVEXA.app; updates install only into the packaged app",
            ));
        }
        Ok(app.to_path_buf())
    } else {
        Ok(exe)
    }
}

fn previous(path: &Path) -> PathBuf {
    let mut name = path.file_name().expect("file name").to_os_string();
    name.push(".previous");
    path.with_file_name(name)
}

fn run(cmd: &mut Command) -> Result<String, InstallError> {
    let out = cmd.output().map_err(|e| err(e.to_string()))?;
    if !out.status.success() {
        return Err(err(String::from_utf8_lossy(&out.stderr).trim().to_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// macOS: unpack the downloaded zip, check the bundle, swap it in.
pub fn install_macos(zip: &Path, app: &Path, release: &str) -> Result<(), InstallError> {
    let parent = app
        .parent()
        .ok_or_else(|| err("app has no parent folder"))?;
    let staging = parent.join(format!(".orvexa-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)
        .map_err(|e| err(format!("the app folder is not writable: {e}")))?;
    let result = (|| {
        run(Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(&staging))?;
        let new_app = staging.join("ORVEXA.app");
        if !new_app.join("Contents/MacOS/ORVEXA").is_file() {
            return Err(err("the update does not contain ORVEXA.app"));
        }
        let plist = new_app.join("Contents/Info.plist");
        let version = run(Command::new("/usr/bin/plutil")
            .args(["-extract", "ORVEXAPreviewVersion", "raw", "-o", "-"])
            .arg(&plist))?;
        if version != release {
            return Err(err(format!("the update is {version}, expected {release}")));
        }
        run(Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict"])
            .arg(&new_app))?;
        let old = previous(app);
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(app, &old).map_err(|e| err(e.to_string()))?;
        if let Err(e) = std::fs::rename(&new_app, app) {
            let _ = std::fs::rename(&old, app);
            return Err(err(e.to_string()));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Windows: the running exe can be renamed but not overwritten.
pub fn install_windows(downloaded: &Path, exe: &Path) -> Result<(), InstallError> {
    let bytes = std::fs::read(downloaded).map_err(|e| err(e.to_string()))?;
    let pe = bytes
        .get(0x3c..0x40)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        .ok_or_else(|| err("not an executable"))?;
    let x64 = bytes
        .get(pe..pe + 6)
        .is_some_and(|h| &h[..4] == b"PE\0\0" && u16::from_le_bytes([h[4], h[5]]) == 0x8664);
    if !x64 {
        return Err(err("the update is not a Windows x64 executable"));
    }
    let old = previous(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| err(e.to_string()))?;
    if let Err(e) = std::fs::copy(downloaded, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(err(e.to_string()));
    }
    Ok(())
}

/// Starts the installed app again after this process exits.
/// The new process gets the same command-line arguments (data folder, port).
#[cfg(target_os = "macos")]
pub fn relaunch(installed: &Path) -> Result<(), InstallError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    Command::new("/bin/sh")
        .args(["-c", "sleep 2; app=\"$0\"; if [ $# -gt 0 ]; then /usr/bin/open -n \"$app\" --args \"$@\"; else /usr/bin/open \"$app\"; fi"])
        .arg(installed)
        .args(args)
        .spawn()
        .map_err(|e| err(e.to_string()))?;
    Ok(())
}

/// Starts the installed app again after this process exits.
#[cfg(windows)]
pub fn relaunch(installed: &Path) -> Result<(), InstallError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let args: String = std::env::args()
        .skip(1)
        .map(|a| format!(" \"{}\"", a.replace('"', "")))
        .collect();
    Command::new("cmd")
        .raw_arg(format!(
            "/C timeout /T 2 /NOBREAK >NUL & start \"\" \"{}\"{args}",
            installed.display()
        ))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| err(e.to_string()))?;
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn relaunch(_installed: &Path) -> Result<(), InstallError> {
    Err(err(
        "self-update is available for the macOS and Windows apps only",
    ))
}

/// Called once the app has started: the previous build is no longer needed.
pub fn finish() {
    if let Ok(path) = installed_path() {
        let old = previous(&path);
        if old.is_dir() {
            let _ = std::fs::remove_dir_all(&old);
        } else if old.is_file() {
            let _ = std::fs::remove_file(&old);
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// A minimal ad-hoc signed bundle with the given preview version.
    fn bundle(dir: &Path, version: &str) -> PathBuf {
        let app = dir.join("ORVEXA.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(
            app.join("Contents/MacOS/ORVEXA"),
            format!("#!/bin/sh\necho {version}\n"),
        )
        .unwrap();
        run(Command::new("/bin/chmod")
            .args(["+x"])
            .arg(app.join("Contents/MacOS/ORVEXA")))
        .unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>ORVEXA</string><key>CFBundleIdentifier</key><string>org.orvexa.test</string><key>ORVEXAPreviewVersion</key><string>{version}</string></dict></plist>"#),
        )
        .unwrap();
        run(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&app))
        .unwrap();
        app
    }

    fn zip(app: &Path, out: &Path) {
        run(Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(out))
        .unwrap();
    }

    #[test]
    fn swaps_bundle_and_keeps_previous() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("Applications");
        std::fs::create_dir(&installed).unwrap();
        let current = bundle(&installed, "0.1.0-desktop-preview.4");
        let build = tmp.path().join("build");
        std::fs::create_dir(&build).unwrap();
        let next = bundle(&build, "0.1.0-desktop-preview.5");
        let archive = tmp.path().join("update.zip");
        zip(&next, &archive);

        assert!(
            install_macos(&archive, &current, "0.1.0-desktop-preview.6").is_err(),
            "version must match the manifest"
        );
        assert!(
            std::fs::read_to_string(current.join("Contents/MacOS/ORVEXA"))
                .unwrap()
                .contains("preview.4"),
            "failed update changes nothing"
        );

        install_macos(&archive, &current, "0.1.0-desktop-preview.5").unwrap();
        assert!(
            std::fs::read_to_string(current.join("Contents/MacOS/ORVEXA"))
                .unwrap()
                .contains("preview.5")
        );
        assert!(
            previous(&current).join("Contents/MacOS/ORVEXA").is_file(),
            "previous build kept for rollback"
        );

        // A bundle whose signature no longer matches is refused.
        std::fs::write(
            next.join("Contents/MacOS/ORVEXA"),
            "#!/bin/sh\necho tampered\n",
        )
        .unwrap();
        let tampered = tmp.path().join("tampered.zip");
        zip(&next, &tampered);
        assert!(install_macos(&tampered, &current, "0.1.0-desktop-preview.5").is_err());
    }
}
