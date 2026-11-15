//! The HogSpasm installer: a small unattended GUI.
//!
//! Double-click -> UAC -> a window opens with a drawn pig face, downloads the
//! latest release from GitHub, installs it, oinks, and closes by itself. No
//! buttons anywhere inside the window. (The secret word still works.)

#![windows_subsystem = "windows"]

use parking_lot::Mutex;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation as fnd;
use windows_sys::Win32::Graphics::Gdi as gdi;
use windows_sys::Win32::System::LibraryLoader as ll;
use windows_sys::Win32::System::Threading as thr;
use windows_sys::Win32::UI::Shell as shell;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

const DOWNLOAD_URL: &str =
    "https://github.com/ada-mmmm/hoggypiggy/releases/latest/download/HogSpasm.vst3.zip";

const CREAM: u32 = 0x00EFF7FC;
const INK: u32 = 0x00302C28;
const SOFT: u32 = 0x009280D8;

// ---------------------------------------------------------------------------
// Shared state between the UI thread and the install worker.
// ---------------------------------------------------------------------------

struct Shared {
    status: Mutex<String>,
    done: Mutex<Option<(bool, Instant)>>,
    stop: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new("warming up the sty...".to_string()),
            done: Mutex::new(None),
            stop: AtomicBool::new(false),
        })
    }

    fn set_status(&self, s: String) {
        *self.status.lock() = s;
    }

    fn finish(&self, success: bool, msg: String) {
        *self.status.lock() = msg;
        *self.done.lock() = Some((success, Instant::now()));
    }
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Entry point: relaunch elevated (UAC, once, before anything opens), then the
// elevated instance shows the window and runs unattended.
// ---------------------------------------------------------------------------

fn main() {
    let elevated = std::env::args().any(|a| a == "--elevated");

    if !elevated {
        // Relaunch ourselves with the runas verb; the UAC prompt appears
        // before the installer window ever opens. If the user declines,
        // nothing happens.
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return,
        };
        unsafe {
            shell::ShellExecuteW(
                std::ptr::null_mut(),
                wide("runas").as_ptr(),
                wide(&exe.to_string_lossy()).as_ptr(),
                wide("--elevated").as_ptr(),
                std::ptr::null(),
                wm::SW_SHOWNORMAL,
            );
        }
        return;
    }

    run_gui();
}

fn run_gui() {
    let shared = Shared::new();
    let _ = SHARED.set(Arc::clone(&shared));

    unsafe {
        let class_name = wide("HogSpasmInstallerWindow\0");
        let wc = wm::WNDCLASSW {
            style: wm::CS_HREDRAW | wm::CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: ll::GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: wm::LoadCursorW(std::ptr::null_mut(), wm::IDC_ARROW as usize as *const u16),
            hbrBackground: gdi::CreateSolidBrush(CREAM),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        wm::RegisterClassW(&wc);

        // Center the window on the primary monitor.
        let screen_w = wm::GetSystemMetrics(wm::SM_CXSCREEN);
        let screen_h = wm::GetSystemMetrics(wm::SM_CYSCREEN);
        let (w, h) = (360, 430);

        let title = wide("HogSpasm Installer");
        let hwnd = wm::CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            wm::WS_OVERLAPPEDWINDOW & !wm::WS_MAXIMIZEBOX & !wm::WS_MINIMIZEBOX,
            (screen_w - w) / 2,
            (screen_h - h) / 2,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ll::GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }

        wm::SetTimer(hwnd, 1, 100, None);
        wm::ShowWindow(hwnd, wm::SW_SHOWNORMAL);

        // The install worker: downloads and installs while the UI breathes.
        {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || worker(shared));
        }

        let mut msg: wm::MSG = std::mem::zeroed();
        while wm::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            wm::TranslateMessage(&msg);
            wm::DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: fnd::HWND,
    msg: u32,
    wparam: fnd::WPARAM,
    lparam: fnd::LPARAM,
) -> fnd::LRESULT {
    match msg {
        wm::WM_TIMER => {
            if let Some(shared) = SHARED.get() {
                if let Some((success, at)) = *shared.done.lock() {
                    let hold = if success { 2500 } else { 6000 };
                    if at.elapsed() > Duration::from_millis(hold) {
                        wm::PostQuitMessage(0);
                    }
                }
            }
            gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        wm::WM_PAINT => {
            paint(hwnd);
            0
        }
        // Clicking anywhere also closes it, just in case someone is impatient.
        wm::WM_LBUTTONDOWN => {
            wm::PostQuitMessage(0);
            0
        }
        wm::WM_DESTROY => {
            wm::PostQuitMessage(0);
            0
        }
        _ => wm::DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// Painting: the pig face, the title, and the live status.
// ---------------------------------------------------------------------------

unsafe fn paint(hwnd: fnd::HWND) {
    let mut ps: gdi::PAINTSTRUCT = std::mem::zeroed();
    let hdc = gdi::BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        return;
    }

    let mut rc: fnd::RECT = std::mem::zeroed();
    wm::GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;

    let bg = gdi::CreateSolidBrush(CREAM);
    let mut bg_rc = fnd::RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    };
    gdi::FillRect(hdc, &bg_rc, bg);
    gdi::DeleteObject(bg);

    // The pig face, big and proud, in the upper half.
    let cx = w as f64 * 0.5;
    let cy = h as f64 * 0.32;
    let rx = w as f64 * 0.32;
    let ry = h as f64 * 0.26;
    draw_pig(hdc, cx, cy, rx, ry);

    // Title.
    draw_text(hdc, "HOGSPASM", fnd::RECT {
        left: 0,
        top: (h as f64 * 0.58) as i32,
        right: w,
        bottom: (h as f64 * 0.66) as i32,
    }, INK, 26, true);

    // Status lines.
    let (status, done) = SHARED
        .get()
        .map(|s| (s.status.lock().clone(), *s.done.lock()))
        .unwrap_or_else(|| ("...".to_string(), None));

    let status_color = match done {
        Some((true, _)) => 0x00187018,  // green ink
        Some((false, _)) => 0x000020C8, // red ink
        None => INK,
    };

    draw_text(hdc, &status, fnd::RECT {
        left: 12,
        top: (h as f64 * 0.70) as i32,
        right: w - 12,
        bottom: (h as f64 * 0.92) as i32,
    }, status_color, 15, false);

    gdi::EndPaint(hwnd, &ps);
}

unsafe fn draw_pig(hdc: gdi::HDC, cx: f64, cy: f64, rx: f64, ry: f64) {
    let pink = 0x00B9A7F4u32;
    let outline = 0x008C78D6u32;
    let snout = 0x009E8CE2u32;
    let nostril = 0x005A4048u32;
    let white = 0x00FFFFFFu32;
    let pupil = 0x00101018u32;
    let cheek = 0x00C0A0F0u32;

    // Ears.
    for side in [-1.0f64, 1.0] {
        paint_polygon(
            hdc,
            &[
                (cx + side * rx * 0.90, cy - ry * 0.30),
                (cx + side * rx * 0.50, cy - ry * 1.00),
                (cx + side * rx * 0.12, cy - ry * 0.52),
            ],
            pink,
            outline,
        );
    }

    // Head.
    paint_ellipse(hdc, cx, cy, rx, ry, pink, outline);

    // Snout + nostrils.
    let snout_y = cy + ry * 0.38;
    paint_ellipse(hdc, cx, snout_y, rx * 0.42, ry * 0.30, snout, 0x008C78D6);
    for side in [-1.0f64, 1.0] {
        paint_ellipse(
            hdc,
            cx + side * rx * 0.16,
            snout_y,
            rx * 0.055,
            rx * 0.055,
            nostril,
            nostril,
        );
    }

    // Eyes.
    let eye_y = cy - ry * 0.18;
    let er = rx * 0.13;
    for side in [-1.0f64, 1.0] {
        paint_ellipse(hdc, cx + side * rx * 0.42, eye_y, er, er, white, outline);
        paint_ellipse(hdc, cx + side * rx * 0.42, eye_y, er * 0.5, er * 0.5, pupil, pupil);
    }

    // Rosy cheeks.
    for side in [-1.0f64, 1.0] {
        paint_ellipse(
            hdc,
            cx + side * rx * 0.58,
            cy + ry * 0.10,
            rx * 0.11,
            ry * 0.08,
            cheek,
            cheek,
        );
    }
}

unsafe fn paint_ellipse(
    hdc: gdi::HDC,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    fill: u32,
    outline: u32,
) {
    let brush = gdi::CreateSolidBrush(fill);
    let pen = gdi::CreatePen(gdi::PS_SOLID, 2, outline);
    let old_brush = gdi::SelectObject(hdc, brush);
    let old_pen = gdi::SelectObject(hdc, pen);
    gdi::Ellipse(
        hdc,
        (cx - rx) as i32,
        (cy - ry) as i32,
        (cx + rx) as i32,
        (cy + ry) as i32,
    );
    gdi::SelectObject(hdc, old_brush);
    gdi::SelectObject(hdc, old_pen);
    gdi::DeleteObject(brush);
    gdi::DeleteObject(pen);
}

unsafe fn paint_polygon(hdc: gdi::HDC, points: &[(f64, f64)], fill: u32, outline: u32) {
    let pts: Vec<fnd::POINT> = points
        .iter()
        .map(|(x, y)| fnd::POINT {
            x: *x as i32,
            y: *y as i32,
        })
        .collect();
    let brush = gdi::CreateSolidBrush(fill);
    let pen = gdi::CreatePen(gdi::PS_SOLID, 2, outline);
    let old_brush = gdi::SelectObject(hdc, brush);
    let old_pen = gdi::SelectObject(hdc, pen);
    gdi::Polygon(hdc, pts.as_ptr(), pts.len() as i32);
    gdi::SelectObject(hdc, old_brush);
    gdi::SelectObject(hdc, old_pen);
    gdi::DeleteObject(brush);
    gdi::DeleteObject(pen);
}

unsafe fn draw_text(hdc: gdi::HDC, text: &str, rect: fnd::RECT, color: u32, height: i32, bold: bool) {
    let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    let font = gdi::CreateFontW(
        height,
        0,
        0,
        0,
        if bold { 700 } else { 400 },
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        0,
        face.as_ptr(),
    );
    let old_font = gdi::SelectObject(hdc, font);
    gdi::SetBkMode(hdc, gdi::TRANSPARENT as i32);
    gdi::SetTextColor(hdc, color);

    let mut wide_text: Vec<u16> = text.encode_utf16().collect();
    let mut r = rect;
    gdi::DrawTextW(
        hdc,
        wide_text.as_mut_ptr(),
        wide_text.len() as i32,
        &mut r,
        gdi::DT_CENTER | gdi::DT_VCENTER | gdi::DT_SINGLELINE,
    );

    gdi::SelectObject(hdc, old_font);
    gdi::DeleteObject(font);
}

// ---------------------------------------------------------------------------
// The install worker (runs on its own thread, feeds status to the window).
// ---------------------------------------------------------------------------

fn worker(shared: Arc<Shared>) {
    match run_install(&shared) {
        Ok(()) => shared.finish(true, "OINK! HogSpasm installed.".to_string()),
        Err(e) => shared.finish(false, format!("OOPS: {e}")),
    }
}

fn run_install(shared: &Arc<Shared>) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("HogSpasm-install");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    // 1. Download.
    shared.set_status("downloading the hog...".to_string());
    let zip_path = tmp.join("HogSpasm.vst3.zip");
    download(DOWNLOAD_URL, &zip_path, shared)?;

    // 2. Extract.
    shared.set_status("unpacking...".to_string());
    let extract_dir = tmp.join("unpacked");
    extract(&zip_path, &extract_dir)?;
    let bundle = find_bundle(&extract_dir).ok_or("bundle not found in the zip")?;

    // 3. Install (we are elevated, so the system folder is writable).
    shared.set_status("installing hogs...".to_string());
    let system_target = PathBuf::from(r"C:\Program Files\Common Files\VST3\HogSpasm.vst3");
    install_bundle(&bundle, &system_target)?;

    if !verify_target(&system_target) {
        return Err("installation could not be verified".to_string());
    }

    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

fn download(url: &str, dest: &Path, shared: &Arc<Shared>) -> Result<(), String> {
    let resp = ureq::get(url).call().map_err(|e| format!("download failed: {e}"))?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
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

        shared.set_status(format!(
            "downloading... {:.1} / {:.1} MB",
            got as f64 / 1e6,
            if total > 0 { total as f64 / 1e6 } else { got as f64 / 1e6 }
        ));
    }
    Ok(())
}

fn extract(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
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
    for entry in fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        let candidate = entry.path().join("HogSpasm.vst3");
        if candidate.join("Contents").is_dir() {
            return Some(candidate);
        }
    }
    None
}

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
                std::thread::sleep(Duration::from_millis(200));
            }
            if !copied {
                return Err(format!("could not copy {}", entry.path().display()));
            }
        }
    }
    Ok(())
}

fn verify_target(target: &Path) -> bool {
    let module = target.join(r"Contents\x86_64-win");
    module.join("HogSpasm.vst3").is_file()
        && module.join("hog_machine.ps1").is_file()
        && module.join("oinks").is_dir()
        && module.join("hogs").is_dir()
}
