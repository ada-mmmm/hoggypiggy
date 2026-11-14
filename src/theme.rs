//! The interior design department: hogifies the wallpaper, the desktop icons,
//! and the autostart configuration — and restores all of it when the secret
//! word is spoken.
//!
//! The heavy lifting (wallpaper via IDesktopWallpaper, icon rewriting) happens
//! in `hog_decor.ps1`, which is shipped next to the plugin and keeps its own
//! state under `%LOCALAPPDATA%\HogSpasm` for the revert. The plugin picks the
//! wallpaper, stages it, and drives the script; the autostart entry is
//! handled here directly.

use rand::Rng;
use std::path::PathBuf;

use windows_sys::Win32::Foundation as fnd;
use windows_sys::Win32::System::Registry as reg;
use windows_sys::Win32::System::Threading as thr;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const AUTOSTART_NAME: &str = "HogSpasm";

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn state_dir() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .ok()
        .map(|l| PathBuf::from(l).join("HogSpasm"))
}

fn theme_is_applied() -> bool {
    state_dir()
        .map(|d| d.join("wallpaper_orig.txt").exists())
        .unwrap_or(false)
}

fn push_images(dir: PathBuf, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
                out.push(path);
            }
        }
    }
}

/// Pick a hog picture for the wallpaper: user-provided pictures first, then
/// the bundled fallback wallpaper.
fn pick_hog_wallpaper() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    unsafe {
        if let Some(dir) = crate::hogs::module_dir() {
            push_images(dir.join("hogs"), &mut candidates);
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        push_images(
            PathBuf::from(userprofile).join("Pictures").join("HogSpasm"),
            &mut candidates,
        );
    }
    unsafe {
        if let Some(dir) = crate::hogs::module_dir() {
            let fallback = dir.join("hog_wallpaper.jpg");
            if fallback.is_file() {
                candidates.push(fallback);
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }
    let mut rng = rand::thread_rng();
    Some(candidates[rng.gen_range(0..candidates.len())].clone())
}

/// Runs the decor script. It handles both wallpaper (via IDesktopWallpaper)
/// and icons, and keeps its own state files for the revert.
fn run_decor_script(mode: &str, wallpaper: Option<&PathBuf>) {
    let Some(dir) = (unsafe { crate::hogs::module_dir() }) else {
        return;
    };
    let script = dir.join("hog_decor.ps1");
    if !script.is_file() {
        return;
    }

    let mut cmd = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\" -Mode {}",
        script.display(),
        mode
    );
    if let Some(wp) = wallpaper {
        cmd.push_str(&format!(" -Wallpaper \"{}\"", wp.display()));
    }
    if mode == "apply" {
        cmd.push_str(&format!(" -Ico \"{}\"", dir.join("hog.ico").display()));
    }

    let mut cmd_w = wide(&cmd);
    unsafe {
        let mut si: thr::STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<thr::STARTUPINFOW>() as u32;
        let mut pi: thr::PROCESS_INFORMATION = std::mem::zeroed();

        let ok = thr::CreateProcessW(
            std::ptr::null(),
            cmd_w.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            thr::CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        );
        if ok != 0 {
            if !pi.hProcess.is_null() {
                fnd::CloseHandle(pi.hProcess);
            }
            if !pi.hThread.is_null() {
                fnd::CloseHandle(pi.hThread);
            }
        }
    }
}

fn ensure_autostart() {
    // The boot-time machine is the hidden PowerShell worker — no exe, so no
    // SmartScreen prompt at startup either.
    let Some(script) = crate::hogs::hog_machine_candidates()
        .into_iter()
        .find(|p| p.is_file())
    else {
        return;
    };

    let cmd = wide(&format!(
        "powershell.exe -WindowStyle Hidden -ExecutionPolicy Bypass -File \"{}\"",
        script.display()
    ));
    unsafe {
        reg::RegSetKeyValueW(
            reg::HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(AUTOSTART_NAME).as_ptr(),
            reg::REG_SZ,
            cmd.as_ptr() as *const core::ffi::c_void,
            (cmd.len() * 2) as u32,
        );
    }
}

fn remove_autostart() {
    unsafe {
        reg::RegDeleteKeyValueW(
            reg::HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(AUTOSTART_NAME).as_ptr(),
        );
    }
}

/// Hogifies the desktop: wallpaper, icons, and the boot-time autostart.
/// Skips everything if a previous hog session was never reverted.
pub fn apply_hog_theme() {
    if theme_is_applied() {
        // Already hogged; the saved original is still waiting for piggy.
        return;
    }

    // Pick a hog picture and stage it where the decor script (and the revert)
    // can always find it.
    let mut staged: Option<PathBuf> = None;
    if let Some(source) = pick_hog_wallpaper() {
        if let Some(dir) = state_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let ext = source
                .extension()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| "jpg".to_string());
            let dest = dir.join(format!("wallpaper_hog.{ext}"));
            if std::fs::copy(&source, &dest).is_ok() {
                staged = Some(dest);
            } else {
                staged = Some(source);
            }
        }
    }

    run_decor_script("apply", staged.as_ref());
    ensure_autostart();
}

/// Puts the desktop back exactly the way it was. Triggered by the secret word.
pub fn revert_hog_theme() {
    run_decor_script("revert", None);
    remove_autostart();
}
