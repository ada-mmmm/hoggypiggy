//! The HogSpasm installer: downloads the latest release bundle from GitHub
//! and installs it. One small exe, everything else comes from the cloud.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const DOWNLOAD_URL: &str =
    "https://github.com/ada-mmmm/hoggypiggy/releases/latest/download/HogSpasm.vst3.zip";

fn main() {
    println!("==============================");
    println!("  HogSpasm installer");
    println!("==============================");
    println!();

    let elevated = std::env::args().any(|a| a == "--elevated");

    match run(elevated) {
        Ok(()) => {}
        Err(e) => {
            eprintln!();
            eprintln!("install failed: {e}");
            std::process::exit(1);
        }
    }

    println!();
    println!("Press Enter to exit...");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

fn run(elevated: bool) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("HogSpasm-install-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    // 1. Download (or reuse the zip the non-elevated instance already got).
    let zip_path = tmp.join("HogSpasm.vst3.zip");
    // Any argument pointing at a real .zip file counts (the elevated instance
    // receives the zip the original instance already downloaded).
    let passed_zip = std::env::args()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip")));

    if let Some(z) = passed_zip {
        println!("Using the bundle the previous step already downloaded...");
        fs::copy(&z, &zip_path).map_err(|e| e.to_string())?;
    } else {
        download(DOWNLOAD_URL, &zip_path)?;
    }

    // 2. Extract.
    let extract_dir = tmp.join("unpacked");
    extract(&zip_path, &extract_dir)?;
    let bundle = find_bundle(&extract_dir).ok_or("HogSpasm.vst3 bundle not found in the zip")?;

    // 3. Install.
    let system_target = PathBuf::from(r"C:\Program Files\Common Files\VST3\HogSpasm.vst3");
    let user_target = std::env::var("LOCALAPPDATA")
        .map(|l| PathBuf::from(l).join(r"Programs\Common\VST3\HogSpasm.vst3"))
        .map_err(|_| "LOCALAPPDATA not set".to_string())?;

    let mut installed = install_bundle(&bundle, &system_target).is_ok()
        && verify_target(&system_target);

    if !installed && !elevated {
        println!();
        println!("The system VST3 folder needs admin rights - asking for elevation...");
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if elevate(&exe, &zip_path) && verify_target(&system_target) {
            installed = true;
            println!("Installed to {}", system_target.display());
        }
    }

    if !installed {
        if elevated {
            return Err(
                "could not write to the system VST3 folder - close FL Studio and try again"
                    .to_string(),
            );
        }
        println!("Installing for this user instead...");
        install_bundle(&bundle, &user_target).map_err(|e| e.to_string())?;
        if !verify_target(&user_target) {
            return Err("installation could not be verified".to_string());
        }
        installed = true;
        println!("Installed to {}", user_target.display());
    }

    let _ = fs::remove_dir_all(&tmp);

    println!();
    println!("HogSpasm is ready. In FL Studio:");
    println!("  1. Options > Manage plugins > Find installed plugins > Start scan");
    println!("  2. Enable HogSpasm in the list");
    println!("  3. Drop it on any mixer track. Oink.");
    Ok(())
}

/// A truthful install: the plugin module, the machine script, and the sounds
/// must all be present.
fn verify_target(target: &Path) -> bool {
    let module = target.join(r"Contents\x86_64-win");
    module.join("HogSpasm.vst3").is_file()
        && module.join("hog_machine.ps1").is_file()
        && module.join("oinks").is_dir()
        && module.join("hogs").is_dir()
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    println!("Downloading the hog from GitHub...");
    let resp = ureq::get(url).call().map_err(|e| format!("download failed: {e}"))?;

    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();

    let mut file = fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut got: u64 = 0;

    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;

        match total {
            Some(t) if t > 0 => print!(
                "\r  {:.1} / {:.1} MB   ",
                got as f64 / 1e6,
                t as f64 / 1e6
            ),
            _ => print!("\r  {:.1} MB   ", got as f64 / 1e6),
        }
        let _ = std::io::stdout().flush();
    }
    println!();
    Ok(())
}

fn extract(zip_path: &Path, dest: &Path) -> Result<(), String> {
    println!("Unpacking...");
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // Zip-slip guard: only accept paths inside the destination.
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out_file = fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out_file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn find_bundle(root: &Path) -> Option<PathBuf> {
    let direct = root.join("HogSpasm.vst3");
    if direct.join("Contents").is_dir() {
        return Some(direct);
    }

    // Fallback: shallow search for the bundle folder.
    for entry in fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        let candidate = entry.path().join("HogSpasm.vst3");
        if candidate.join("Contents").is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Merge-copies the bundle over whatever is at the target. Stale leftovers
/// from older versions are tolerated; every file gets a couple of retries to
/// survive antivirus scans briefly locking a file.
fn install_bundle(bundle: &Path, target: &Path) -> Result<(), String> {
    copy_dir(bundle, target).map_err(|e| e.to_string())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let to = dst.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            let mut copied = false;
            for _ in 0..3 {
                if fs::copy(entry.path(), &to).is_ok() {
                    copied = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            if !copied {
                return Err(format!("could not copy {}", entry.path().display()));
            }
        }
    }
    Ok(())
}

/// Relaunches ourselves elevated (UAC prompt) and waits for it to finish.
/// The elevated instance receives the already-downloaded zip and the
/// --elevated flag so it never re-elevates. The wrapper propagates the child
/// exit code so a failed elevated run is never mistaken for success.
fn elevate(exe: &Path, zip_path: &Path) -> bool {
    let script = format!(
        "$p = Start-Process -FilePath '{}' -ArgumentList '--elevated','{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        exe.display().to_string().replace('\'', "''"),
        zip_path.display().to_string().replace('\'', "''")
    );

    match std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}
