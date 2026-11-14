//! The hog department: spawns, paints, and corrals popup hogs.
//!
//! Hogs prefer real photographs. Any `.png`/`.jpg`/`.jpeg`/`.bmp` dropped in
//! the `hogs` folder next to the plugin, or in `%USERPROFILE%\Pictures\HogSpasm`,
//! gets used. If no pictures can be found, we fall back to hand-drawn GDI
//! cartoon hogs, which are still quite handsome.
//!
//! Hogs never leave on their own. They can only be evicted with the X on their
//! title bar or the CLEAR ALL HOGS button.

use image::DynamicImage;
use parking_lot::Mutex;
use rand::seq::SliceRandom;
use rand::Rng;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation as fnd;
use windows_sys::Win32::Graphics::Gdi as gdi;
use windows_sys::Win32::System::LibraryLoader as ll;
use windows_sys::Win32::System::Threading as thr;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

/// An HWND that is safe to share between threads. Handles are process-global,
/// and we only ever touch them through `PostMessageW` (thread-safe).
#[derive(Clone, Copy, PartialEq, Eq)]
struct HogWindow(fnd::HWND);
unsafe impl Send for HogWindow {}
unsafe impl Sync for HogWindow {}

/// Shared state between the plugin, its editor, and the hog spawner.
pub struct HogShared {
    hogs: Mutex<Vec<HogWindow>>,
    /// Set when the secret word has been spoken (or the plugin is going away):
    /// no new hogs may be born.
    pub stop: AtomicBool,
}

const CREAM: u32 = 0x00EFF7FC; // COLORREF: rgb(252, 247, 239)
const CREAM_BYTES: [u8; 3] = [252, 247, 239];

impl HogShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            hogs: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
        })
    }

    fn add(&self, hwnd: fnd::HWND) {
        self.hogs.lock().push(HogWindow(hwnd));
    }

    fn remove(&self, hwnd: fnd::HWND) {
        self.hogs.lock().retain(|hog| hog.0 != hwnd);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mood {
    Normal,
    Angry,
    Sleepy,
    Shocked,
    Derpy,
    Fancy,
}

/// A decoded hog photograph, ready for StretchDIBits.
pub struct HogImage {
    width: i32,
    height: i32,
    /// RGBA8, top-down rows, alpha flattened onto the popup background.
    pixels: Vec<u8>,
}

unsafe impl Send for HogImage {}
unsafe impl Sync for HogImage {}

struct HogDesign {
    width: i32,
    height: i32,
    title: String,
    caption: String,
    body: u32,
    outline: u32,
    mood: Mood,
    hat: bool,
    hat_color: u32,
    mud: bool,
    image: Option<Arc<HogImage>>,
    flipped: bool,
}

const TITLES: &[&str] = &[
    "A wild hog appears!",
    "HOG",
    "H O G",
    "WARNING: HOG",
    "certified hog",
    "hog alert",
    "OINKEMENTS",
    "untitled hog project",
    "do not feed the hog",
    "hog.exe has stopped oinking",
    "hog_042_final_FINAL.jpg",
    "HOG DETECTED",
    "this hog is now yours",
    "hog, permanent",
];

const CAPTIONS: &[&str] = &[
    "OINK",
    "oink oink",
    "SQUEEE",
    "HONK",
    "snacc?",
    "me rn",
    "is it mud season yet",
    "10/10 would root",
    "BACON IS CANCELLED",
    "insert mud here",
    "hog intensifies",
    "do NOT boop",
    "*sniff sniff*",
    "professional hog",
    "mud quality: excellent",
    "oink.",
    "IMPORTANT HOG",
    "here to stay",
    "your problem now",
];

impl HogDesign {
    fn random(rng: &mut impl Rng) -> Self {
        // (fill, outline) COLORREF pairs: piglet pink, brown, grey, chalk white, void hog.
        // (Cartoon hogs only, but a pig deserves options.)
        const PALETTES: &[(u32, u32)] = &[
            (0x00B9A7F4, 0x008C78D6),
            (0x00527CA6, 0x00385878),
            (0x00B8B0B0, 0x008C8282),
            (0x00E4ECF0, 0x00A09890),
            (0x003C4248, 0x00181412),
        ];

        const HAT_COLORS: &[u32] = &[
            0x00E0B040, // teal
            0x00E07840, // orange
            0x00C040B0, // magenta
            0x0040B0E0, // blue
        ];

        let mood = match rng.gen_range(0..6) {
            0 => Mood::Normal,
            1 => Mood::Angry,
            2 => Mood::Sleepy,
            3 => Mood::Shocked,
            4 => Mood::Derpy,
            _ => Mood::Fancy,
        };

        let (body, outline) = PALETTES[rng.gen_range(0..PALETTES.len())];

        // Real photographs take priority; cartoons are the emergency backup.
        let images = hog_images();
        let image = if !images.is_empty() && rng.gen_bool(0.92) {
            Some(images[rng.gen_range(0..images.len())].clone())
        } else {
            None
        };
        let flipped = rng.gen_bool(0.5);

        let (width, height) = match &image {
            Some(img) => {
                let client_w = rng.gen_range(160..=420);
                let aspect = img.height as f64 / img.width as f64;
                let client_h = ((client_w as f64 * aspect) as i32).clamp(80, 480);
                (client_w, client_h + 30 + 36)
            }
            None => {
                let client_w = rng.gen_range(120..=300);
                (client_w, client_w * 4 / 5 + 30 + 36)
            }
        };

        Self {
            width,
            height,
            title: (*TITLES.choose(rng).unwrap()).to_string(),
            caption: (*CAPTIONS.choose(rng).unwrap()).to_string(),
            body,
            outline,
            mood,
            // Fancy hogs always dress for the occasion.
            hat: mood == Mood::Fancy || rng.gen_bool(0.25),
            hat_color: HAT_COLORS[rng.gen_range(0..HAT_COLORS.len())],
            mud: rng.gen_bool(0.35),
            image,
            flipped,
        }
    }
}

/// Spawns one hog immediately. Call from any thread.
pub fn spawn_hog(shared: &Arc<HogShared>) {
    let shared = Arc::clone(shared);
    thread::spawn(move || unsafe {
        run_hog_window(shared);
    });
}

/// Politely shoos away every currently alive hog. Call from any thread.
pub fn clear_hogs(shared: &HogShared) {
    let hogs: Vec<HogWindow> = shared.hogs.lock().drain(..).collect();
    for hog in hogs {
        unsafe {
            wm::PostMessageW(hog.0, wm::WM_CLOSE, 0, 0);
        }
    }
}

/// THE EMERGENCY OINK. Sets the stop flag, evicts every hog (including any
/// that are still being born), shuts down the Hog Notification Machine, and
/// reverts the whole hog environment — wallpaper, icons, autostart.
/// Triggered by the secret word.
pub fn panic_stop(shared: &Arc<HogShared>) {
    shared.stop.store(true, Ordering::SeqCst);
    clear_hogs(shared);
    kill_hog_machine();
    crate::theme::revert_hog_theme();

    // Hogs that were mid-spawn may register themselves after the sweep above,
    // so keep sweeping for a moment until everything has settled.
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        for _ in 0..4 {
            thread::sleep(Duration::from_millis(250));
            if !shared.stop.load(Ordering::Relaxed) {
                break;
            }
            clear_hogs(&shared);
        }
    });
}

/// Plugin teardown without the full theme revert: popups cleared and the
/// machine shut down, but the wallpaper/icons/autostart stay until piggy.
pub fn shutdown(shared: &Arc<HogShared>) {
    shared.stop.store(true, Ordering::SeqCst);
    clear_hogs(shared);
    kill_hog_machine();
}

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn state_dir() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .ok()
        .map(|l| PathBuf::from(l).join("HogSpasm"))
}

fn machine_heartbeat_path() -> Option<PathBuf> {
    state_dir().map(|d| d.join("machine_alive"))
}

/// Candidate locations for the Hog Notification Machine script: next to the
/// plugin (the installer ships it there), then the project folder for dev.
pub fn hog_machine_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    unsafe {
        if let Some(dir) = module_dir() {
            candidates.push(dir.join("hog_machine.ps1"));
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        candidates.push(PathBuf::from(userprofile).join("hog-spasm").join("hog_machine.ps1"));
    }
    candidates
}

/// Runs a command with no visible window. Returns true if the process was
/// created.
fn run_hidden_command(cmd: &str) -> bool {
    let mut cmd_w = wide(cmd);
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
        ok != 0
    }
}

/// The notification machine is a hidden PowerShell worker that touches a
/// heartbeat file every loop. A fresh heartbeat means it is alive.
fn hog_machine_running() -> bool {
    let Some(path) = machine_heartbeat_path() else {
        return false;
    };
    match std::fs::metadata(&path) {
        Ok(meta) => meta
            .modified()
            .ok()
            .map(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(20))
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Launches the Hog Notification Machine: a hidden PowerShell worker showing
/// pig fact toasts and playing oink sounds. No exe, no SmartScreen prompt.
fn launch_hog_machine() -> bool {
    let Some(script) = hog_machine_candidates().into_iter().find(|p| p.is_file()) else {
        return false;
    };
    let cmd = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"",
        script.display()
    );
    run_hidden_command(&cmd)
}

/// Shuts the Hog Notification Machine down, wherever it came from (plugin or
/// boot-time autostart). The plugin launched it, so the plugin gets to
/// unlaunch it.
fn kill_hog_machine() {
    let cmd = "powershell.exe -NoProfile -Command \"Get-CimInstance Win32_Process -Filter \\\"Name='powershell.exe'\\\" | Where-Object { $_.CommandLine -like '*hog_machine.ps1*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }\"";
    run_hidden_command(cmd);
    if let Some(hb) = machine_heartbeat_path() {
        let _ = std::fs::remove_file(hb);
    }
}

/// The background loop. Lingers briefly after the plugin loads, then LAUNCHES
/// the Hog Notification Machine itself — pig fact toasts, oink sounds, and the
/// ~20 hogs per second popup blizzard all begin together. If the machine ever
/// stops, the plugin brings it back. The secret word stops everything.
pub fn spawn_loop(shared: Arc<HogShared>) {
    let mut rng = rand::thread_rng();

    // Linger briefly so plugin scanners (which load the plugin briefly and
    // then drop it) never summon the hog.
    let naps = rng.gen_range(8..=14);
    for _ in 0..naps {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(Duration::from_millis(1000));
    }

    // Unleash the full hog environment: wallpaper, desktop icons, autostart.
    crate::theme::apply_hog_theme();

    let mut failed_launches = 0u32;
    let mut machine_running = hog_machine_running();
    let mut last_poll = Instant::now();

    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }

        // Poll the process list at most every 500 ms.
        if last_poll.elapsed() >= Duration::from_millis(500) {
            last_poll = Instant::now();
            let was_running = machine_running;
            machine_running = hog_machine_running();

            // The machine vanished (tray exit, crash): round up the livestock.
            // The plugin is the launcher now, so it will bring the machine
            // right back.
            if was_running && !machine_running {
                clear_hogs(&shared);
            }
        }

        if !machine_running {
            // Launch the Hog Notification Machine ourselves.
            if failed_launches < 5 && launch_hog_machine() {
                failed_launches = 0;
            } else {
                failed_launches += 1;
            }

            // Give it a moment to come up, checking frequently so the spasm
            // starts as soon as the pig pics do.
            for _ in 0..100 {
                if shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
                if hog_machine_running() {
                    break;
                }
            }
            machine_running = hog_machine_running();
            last_poll = Instant::now();
            continue;
        }

        // ~20 hogs per second, with the occasional mini-herd.
        let interval = (1.0 / 20.0) * rng.gen_range(0.5..1.5);
        thread::sleep(Duration::from_secs_f64(interval));

        if shared.stop.load(Ordering::Relaxed) {
            break;
        }

        let count = if rng.gen_bool(0.05) { 2 } else { 1 };
        for _ in 0..count {
            if shared.stop.load(Ordering::Relaxed) {
                break;
            }
            spawn_hog(&shared);
        }
    }

    // Round up everything on the way out: the popups and the machine alike.
    clear_hogs(&shared);
    kill_hog_machine();
}

// ---------------------------------------------------------------------------
// The secret oink code: type "piggy" anywhere on the system and the herd is
// evacuated immediately. Implemented as a low-level keyboard hook that only
// *observes* keystrokes (it never blocks or eats them), running on its own
// message-pump thread for as long as the plugin is loaded.
// ---------------------------------------------------------------------------

static PIGGY_BUFFER: Mutex<String> = Mutex::new(String::new());
static PIGGY_TARGETS: Mutex<Vec<Arc<HogShared>>> = Mutex::new(Vec::new());
static PIGGY_THREAD_ID: Mutex<Option<u32>> = Mutex::new(None);
static PIGGY_LISTENER: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);

unsafe extern "system" fn piggy_hook_proc(
    code: i32,
    wparam: fnd::WPARAM,
    lparam: fnd::LPARAM,
) -> fnd::LRESULT {
    if code >= 0 {
        let down = wparam as u32 == wm::WM_KEYDOWN || wparam as u32 == wm::WM_SYSKEYDOWN;
        if down {
            let info = &*(lparam as *const wm::KBDLLHOOKSTRUCT);
            if (0x41..=0x5A).contains(&info.vkCode) {
                let letter = (info.vkCode - 0x41) as u8 + b'a';

                let mut buffer = PIGGY_BUFFER.lock();
                buffer.push(letter as char);
                if buffer.len() > 16 {
                    let cut = buffer.len() - 16;
                    buffer.drain(..cut);
                }

                if buffer.ends_with("piggy") {
                    buffer.clear();
                    // The secret word has been spoken. Evacuate every herd.
                    for shared in PIGGY_TARGETS.lock().clone() {
                        panic_stop(&shared);
                    }
                }
            }
        }
    }

    // Never eat keystrokes. The hook is a watcher, not a hog.
    wm::CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Installs the system-wide secret word listener (once per process) and adds
/// this plugin instance's herd to the evacuation list.
pub fn register_piggy_target(shared: Arc<HogShared>) {
    {
        let mut targets = PIGGY_TARGETS.lock();
        if targets.iter().any(|s| Arc::ptr_eq(s, &shared)) {
            return;
        }
        targets.push(shared);
    }

    let mut listener = PIGGY_LISTENER.lock();
    if listener.is_none() {
        *listener = Some(thread::spawn(|| unsafe {
            *PIGGY_THREAD_ID.lock() = Some(thr::GetCurrentThreadId());

            let hook = wm::SetWindowsHookExW(
                wm::WH_KEYBOARD_LL,
                Some(piggy_hook_proc),
                std::ptr::null_mut(),
                0,
            );
            if hook.is_null() {
                return;
            }

            let mut msg: wm::MSG = std::mem::zeroed();
            while wm::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}

            wm::UnhookWindowsHookEx(hook);
        }));
    }
}

/// Removes this instance from the evacuation list and shuts the listener down
/// if no herds remain — the hook must never outlive the plugin's DLL.
pub fn unregister_piggy_target(shared: &Arc<HogShared>) {
    PIGGY_TARGETS.lock().retain(|s| !Arc::ptr_eq(s, shared));

    if PIGGY_TARGETS.lock().is_empty() {
        if let Some(handle) = PIGGY_LISTENER.lock().take() {
            // Wait briefly for the hook thread to announce its thread id.
            let mut tid = *PIGGY_THREAD_ID.lock();
            for _ in 0..20 {
                if tid.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(25));
                tid = *PIGGY_THREAD_ID.lock();
            }
            if let Some(tid) = tid {
                unsafe {
                    wm::PostThreadMessageW(tid, wm::WM_QUIT, 0, 0);
                }
            }
            let _ = handle.join();
            *PIGGY_THREAD_ID.lock() = None;
        }
    }
}

static CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static IMAGE_CACHE: Mutex<Option<(Vec<Arc<HogImage>>, Instant)>> = Mutex::new(None);

thread_local! {
    /// Each hog lives on its own thread, so the current hog's design rides along in TLS.
    static CURRENT_HOG: RefCell<Option<(HogDesign, Arc<HogShared>)>> = RefCell::new(None);
}

fn class_name() -> *const u16 {
    CLASS_NAME
        .get_or_init(|| "HogSpasmHogWindow\0".encode_utf16().collect())
        .as_ptr()
}

unsafe fn ensure_class_registered() {
    CLASS_REGISTERED.get_or_init(|| {
        let wc = wm::WNDCLASSW {
            style: wm::CS_HREDRAW | wm::CS_VREDRAW,
            lpfnWndProc: Some(hog_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: ll::GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: wm::LoadCursorW(std::ptr::null_mut(), wm::IDC_ARROW as usize as *const u16),
            hbrBackground: gdi::CreateSolidBrush(CREAM),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name(),
        };
        wm::RegisterClassW(&wc);
    });
}

/// The directory this plugin's DLL lives in.
pub unsafe fn module_dir() -> Option<PathBuf> {
    let wndproc: unsafe extern "system" fn(fnd::HWND, u32, fnd::WPARAM, fnd::LPARAM) -> fnd::LRESULT =
        hog_wndproc;

    let mut module: fnd::HMODULE = std::ptr::null_mut();
    let flags = ll::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
        | ll::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if ll::GetModuleHandleExW(flags, wndproc as usize as *const u16, &mut module) == 0 {
        return None;
    }

    let mut buf = [0u16; 1024];
    let len = ll::GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }

    let path = String::from_utf16_lossy(&buf[..len]);
    PathBuf::from(path).parent().map(|p| p.to_path_buf())
}

fn load_hog_images() -> Vec<Arc<HogImage>> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // 1. A `hogs` folder next to the plugin module (installed with the plugin).
    unsafe {
        if let Some(dir) = module_dir() {
            dirs.push(dir.join("hogs"));
        }
    }
    // 2. An easy drop-zone: %USERPROFILE%\Pictures\HogSpasm. Pictures dropped
    //    here are picked up without reinstalling anything.
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(userprofile).join("Pictures").join("HogSpasm"));
    }

    for dir in dirs {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut images = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
                continue;
            }

            let loaded: Result<DynamicImage, _> = image::open(&path);
            if let Ok(img) = loaded {
                let mut rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                if width < 16 || height < 16 || width > 4096 || height > 4096 {
                    continue;
                }

                // GDI's StretchDIBits ignores alpha, so flatten any
                // transparency onto the popup background instead of letting
                // it turn black.
                for px in rgba.chunks_exact_mut(4) {
                    let a = px[3] as u32;
                    if a < 255 {
                        for c in 0..3 {
                            px[c] = ((px[c] as u32 * a + CREAM_BYTES[c] as u32 * (255 - a)) / 255)
                                as u8;
                        }
                    }
                    px[3] = 255;
                }

                images.push(Arc::new(HogImage {
                    width: width as i32,
                    height: height as i32,
                    pixels: rgba.into_raw(),
                }));
            }
        }

        if !images.is_empty() {
            return images;
        }
    }

    Vec::new()
}

/// Cached hog photographs. Rescans at most every 2 seconds while we have no
/// images, so pictures dropped into `Pictures\HogSpasm` get picked up live.
fn hog_images() -> Vec<Arc<HogImage>> {
    let mut guard = IMAGE_CACHE.lock();
    match guard.as_ref() {
        Some((images, _)) if !images.is_empty() => return images.clone(),
        _ => {}
    }
    if let Some((_, last_scan)) = guard.as_ref() {
        if last_scan.elapsed() < Duration::from_secs(2) {
            return Vec::new();
        }
    }

    let images = load_hog_images();
    *guard = Some((images.clone(), Instant::now()));
    images
}

unsafe fn run_hog_window(shared: Arc<HogShared>) {
    // Hogs conceived after the secret word was spoken are never born.
    if shared.stop.load(Ordering::Relaxed) {
        return;
    }

    ensure_class_registered();
    let mut rng = rand::thread_rng();
    let hog = HogDesign::random(&mut rng);

    let (x, y) = pick_position(&hog, &mut rng);

    // NOACTIVATE so hogs never steal focus, TOOLWINDOW so they stay out of the
    // taskbar/alt-tab, TOPMOST so the whole screen belongs to the herd.
    let ex_style = wm::WS_EX_TOPMOST | wm::WS_EX_TOOLWINDOW | wm::WS_EX_NOACTIVATE;
    // Caption + close button so hogs are draggable and individually evictable.
    let style = wm::WS_POPUP | wm::WS_CAPTION | wm::WS_SYSMENU;

    let title: Vec<u16> = format!("{}\0", hog.title).encode_utf16().collect();
    let hwnd = wm::CreateWindowExW(
        ex_style,
        class_name(),
        title.as_ptr(),
        style,
        x,
        y,
        hog.width,
        hog.height,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        ll::GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    if hwnd.is_null() {
        return;
    }

    shared.add(hwnd);
    CURRENT_HOG.with(|cell| *cell.borrow_mut() = Some((hog, Arc::clone(&shared))));

    // No timers. No expiry. This hog has signed a permanent lease.
    wm::ShowWindow(hwnd, wm::SW_SHOWNOACTIVATE);

    let mut msg: wm::MSG = std::mem::zeroed();
    while wm::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        wm::TranslateMessage(&msg);
        wm::DispatchMessageW(&msg);
    }

    shared.remove(hwnd);
    CURRENT_HOG.with(|cell| *cell.borrow_mut() = None);
}

fn pick_position(hog: &HogDesign, rng: &mut impl Rng) -> (i32, i32) {
    unsafe {
        // Anywhere on the virtual desktop. The whole screen is hog country.
        let vx = wm::GetSystemMetrics(wm::SM_XVIRTUALSCREEN);
        let vy = wm::GetSystemMetrics(wm::SM_YVIRTUALSCREEN);
        let vw = wm::GetSystemMetrics(wm::SM_CXVIRTUALSCREEN).max(hog.width + 1);
        let vh = wm::GetSystemMetrics(wm::SM_CYVIRTUALSCREEN).max(hog.height + 1);
        (
            vx + rng.gen_range(0..(vw - hog.width).max(1)),
            vy + rng.gen_range(0..(vh - hog.height).max(1)),
        )
    }
}

unsafe extern "system" fn hog_wndproc(
    hwnd: fnd::HWND,
    msg: u32,
    wparam: fnd::WPARAM,
    lparam: fnd::LPARAM,
) -> fnd::LRESULT {
    match msg {
        wm::WM_PAINT => {
            paint_hog(hwnd);
            0
        }
        wm::WM_CLOSE => {
            wm::DestroyWindow(hwnd);
            0
        }
        wm::WM_DESTROY => {
            CURRENT_HOG.with(|cell| {
                if let Some((_, shared)) = cell.borrow().as_ref() {
                    shared.remove(hwnd);
                }
            });
            wm::PostQuitMessage(0);
            0
        }
        // Clicking a hog no longer dismisses it. The hog stays. The hog is
        // forever. Use the X or the CLEAR ALL HOGS button.
        _ => wm::DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_hog(hwnd: fnd::HWND) {
    let mut ps: gdi::PAINTSTRUCT = std::mem::zeroed();
    let hdc = gdi::BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        return;
    }

    let mut rc: fnd::RECT = std::mem::zeroed();
    wm::GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;

    CURRENT_HOG.with(|cell| {
        if let Some((hog, _)) = cell.borrow().as_ref() {
            draw_hog(hdc, w, h, hog);
        }
    });

    gdi::EndPaint(hwnd, &ps);
}

/// Draws an ellipse with the given fill and outline colors.
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

unsafe fn paint_line(hdc: gdi::HDC, x1: f64, y1: f64, x2: f64, y2: f64, color: u32, width: i32) {
    let pen = gdi::CreatePen(gdi::PS_SOLID, width, color);
    let old_pen = gdi::SelectObject(hdc, pen);
    gdi::MoveToEx(hdc, x1 as i32, y1 as i32, std::ptr::null_mut());
    gdi::LineTo(hdc, x2 as i32, y2 as i32);
    gdi::SelectObject(hdc, old_pen);
    gdi::DeleteObject(pen);
}

unsafe fn paint_text(
    hdc: gdi::HDC,
    text: &str,
    rect: fnd::RECT,
    color: u32,
    height: i32,
    bold: bool,
) {
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
        0, // DEFAULT_CHARSET
        0, // OUT_DEFAULT_PRECIS
        0, // CLIP_DEFAULT_PRECIS
        4, // ANTIALIASED_QUALITY
        0,
        face.as_ptr(),
    );
    let old_font = gdi::SelectObject(hdc, font);
    gdi::SetBkMode(hdc, gdi::TRANSPARENT as i32);
    gdi::SetTextColor(hdc, color);

    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut r = rect;
    gdi::DrawTextW(
        hdc,
        wide.as_mut_ptr(),
        wide.len() as i32,
        &mut r,
        gdi::DT_CENTER | gdi::DT_VCENTER | gdi::DT_SINGLELINE,
    );

    gdi::SelectObject(hdc, old_font);
    gdi::DeleteObject(font);
}

unsafe fn draw_hog(hdc: gdi::HDC, w: i32, h: i32, hog: &HogDesign) {
    // Background (same cream as the class brush, just to be sure).
    let bg = gdi::CreateSolidBrush(CREAM);
    let rc = fnd::RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    };
    gdi::FillRect(hdc, &rc, bg);
    gdi::DeleteObject(bg);

    let caption_h = 28;
    let usable = (h - caption_h).max(10);

    match hog.image.as_ref() {
        Some(img) => draw_image_hog(hdc, w, usable, hog, img),
        None => draw_cartoon_hog(hdc, w, usable, hog),
    }

    // Caption strip along the bottom.
    paint_line(
        hdc,
        0.0,
        (h - caption_h) as f64,
        w as f64,
        (h - caption_h) as f64,
        0x00D8C8D0,
        2,
    );
    paint_text(
        hdc,
        &hog.caption,
        fnd::RECT {
            left: 0,
            top: h - caption_h,
            right: w,
            bottom: h,
        },
        0x00302C28,
        (caption_h as f64 * 0.62) as i32,
        true,
    );
}

/// Draws a real hog photograph, fitted and centered, with an optional mirror
/// flip for maximumhog.
unsafe fn draw_image_hog(hdc: gdi::HDC, w: i32, usable: i32, hog: &HogDesign, img: &HogImage) {
    let scale = (w as f64 / img.width as f64).min(usable as f64 / img.height as f64);
    let dst_w = (img.width as f64 * scale) as i32;
    let dst_h = (img.height as f64 * scale) as i32;
    let dx = (w - dst_w) / 2;
    let dy = (usable - dst_h) / 2;

    // Mirror the image if this hog feels like facing the other way.
    let flipped_pixels;
    let pixels: &[u8] = if hog.flipped {
        let row_len = img.width as usize * 4;
        let mut flipped = img.pixels.clone();
        for (dst_row, src_row) in flipped
            .chunks_exact_mut(row_len)
            .zip(img.pixels.chunks_exact(row_len))
        {
            let width = img.width as usize;
            for x in 0..width {
                dst_row[x * 4..x * 4 + 4].copy_from_slice(&src_row[(width - 1 - x) * 4..(width - x) * 4]);
            }
        }
        flipped_pixels = flipped;
        &flipped_pixels
    } else {
        &img.pixels
    };

    let mut bmi: gdi::BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<gdi::BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = img.width;
    bmi.bmiHeader.biHeight = -img.height; // negative = top-down rows
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = gdi::BI_RGB;

    gdi::StretchDIBits(
        hdc,
        dx,
        dy,
        dst_w,
        dst_h,
        0,
        0,
        img.width,
        img.height,
        pixels.as_ptr() as *const core::ffi::c_void,
        &bmi,
        gdi::DIB_RGB_COLORS,
        gdi::SRCCOPY,
    );

    // A tasteful frame around the photograph.
    let brush = gdi::CreateSolidBrush(hog.outline);
    let frame = fnd::RECT {
        left: dx,
        top: dy,
        right: dx + dst_w,
        bottom: dy + dst_h,
    };
    gdi::FrameRect(hdc, &frame, brush);
    gdi::DeleteObject(brush);
}

/// The emergency backup hog, hand-drawn with pure GDI primitives.
unsafe fn draw_cartoon_hog(hdc: gdi::HDC, w: i32, usable: i32, hog: &HogDesign) {
    let wf = w as f64;
    let uf = usable as f64;

    // The hog itself.
    let cx = wf * 0.5;
    let cy = uf * 0.54;
    let rx = wf * 0.34;
    let ry = uf * 0.36;

    // Optional mud puddle to stand in.
    if hog.mud {
        paint_ellipse(hdc, cx, uf * 0.92, rx * 1.15, uf * 0.10, 0x00506E8C, 0x00385048);
    }

    // Ears.
    paint_polygon(
        hdc,
        &[
            (cx - rx * 0.90, cy - ry * 0.30),
            (cx - rx * 0.50, cy - ry * 1.00),
            (cx - rx * 0.12, cy - ry * 0.52),
        ],
        hog.body,
        hog.outline,
    );
    paint_polygon(
        hdc,
        &[
            (cx + rx * 0.90, cy - ry * 0.30),
            (cx + rx * 0.50, cy - ry * 1.00),
            (cx + rx * 0.12, cy - ry * 0.52),
        ],
        hog.body,
        hog.outline,
    );

    // Head.
    paint_ellipse(hdc, cx, cy, rx, ry, hog.body, hog.outline);

    // Party hat (drawn on top of the head).
    if hog.hat {
        paint_polygon(
            hdc,
            &[
                (cx, cy - ry * 1.55),
                (cx - rx * 0.38, cy - ry * 0.72),
                (cx + rx * 0.38, cy - ry * 0.72),
            ],
            hog.hat_color,
            0x00302C28,
        );
        paint_ellipse(hdc, cx, cy - ry * 1.55, rx * 0.10, rx * 0.10, 0x00F0C0C0, 0x00302C28);
    }

    // Shocked hogs gasp before the snout is drawn so it reads as a mouth.
    if hog.mood == Mood::Shocked {
        paint_ellipse(hdc, cx, cy + ry * 0.30, rx * 0.20, ry * 0.22, 0x00302028, 0x00302028);
    }

    // Snout + nostrils.
    let snout_y = cy + ry * 0.38;
    paint_ellipse(hdc, cx, snout_y, rx * 0.42, ry * 0.30, 0x009E8CE2, 0x008C78D6);
    paint_ellipse(hdc, cx - rx * 0.16, snout_y, rx * 0.055, rx * 0.055, 0x005A4048, 0x005A4048);
    paint_ellipse(hdc, cx + rx * 0.16, snout_y, rx * 0.055, rx * 0.055, 0x005A4048, 0x005A4048);

    // Eyes.
    let eye_y = cy - ry * 0.18;
    let eye_dx = rx * 0.42;
    let er = rx * 0.13;
    paint_ellipse(hdc, cx - eye_dx, eye_y, er, er, 0x00FFFFFF, hog.outline);
    paint_ellipse(hdc, cx + eye_dx, eye_y, er, er, 0x00FFFFFF, hog.outline);

    let pupil = 0x00101018u32;
    match hog.mood {
        Mood::Normal | Mood::Fancy => {
            paint_ellipse(hdc, cx - eye_dx, eye_y, er * 0.5, er * 0.5, pupil, pupil);
            paint_ellipse(hdc, cx + eye_dx, eye_y, er * 0.5, er * 0.5, pupil, pupil);
            // Rosy cheeks.
            paint_ellipse(hdc, cx - rx * 0.58, cy + ry * 0.10, rx * 0.11, ry * 0.08, 0x00C0A0F0, 0x00C0A0F0);
            paint_ellipse(hdc, cx + rx * 0.58, cy + ry * 0.10, rx * 0.11, ry * 0.08, 0x00C0A0F0, 0x00C0A0F0);

            if hog.mood == Mood::Fancy {
                // A dapper bow tie below the snout.
                let tie = 0x000030D6u32; // rgb(214, 48, 0)
                let tie_y = snout_y + ry * 0.42;
                paint_polygon(
                    hdc,
                    &[
                        (cx, tie_y),
                        (cx - rx * 0.22, tie_y + ry * 0.13),
                        (cx - rx * 0.22, tie_y - ry * 0.17),
                    ],
                    tie,
                    0x00302C28,
                );
                paint_polygon(
                    hdc,
                    &[
                        (cx, tie_y),
                        (cx + rx * 0.22, tie_y + ry * 0.13),
                        (cx + rx * 0.22, tie_y - ry * 0.17),
                    ],
                    tie,
                    0x00302C28,
                );
                paint_ellipse(hdc, cx, tie_y, rx * 0.06, ry * 0.07, tie, 0x00302C28);
            }
        }
        Mood::Angry => {
            // Small angry pupils + fierce brows.
            paint_ellipse(hdc, cx - eye_dx, eye_y, er * 0.35, er * 0.35, pupil, pupil);
            paint_ellipse(hdc, cx + eye_dx, eye_y, er * 0.35, er * 0.35, pupil, pupil);
            let lw = (w / 50).max(2);
            paint_line(
                hdc,
                cx - eye_dx - er,
                eye_y - er * 1.6,
                cx - eye_dx + er * 0.7,
                eye_y - er * 0.5,
                0x00002A8C,
                lw,
            );
            paint_line(
                hdc,
                cx + eye_dx + er,
                eye_y - er * 1.6,
                cx + eye_dx - er * 0.7,
                eye_y - er * 0.5,
                0x00002A8C,
                lw,
            );
        }
        Mood::Sleepy => {
            // Pupils peeking from the top, then eyelids over the bottom half.
            paint_ellipse(hdc, cx - eye_dx, eye_y - er * 0.4, er * 0.5, er * 0.5, pupil, pupil);
            paint_ellipse(hdc, cx + eye_dx, eye_y - er * 0.4, er * 0.5, er * 0.5, pupil, pupil);
            let lid = gdi::CreateSolidBrush(hog.body);
            let lid_rect = fnd::RECT {
                left: (cx - eye_dx - er) as i32,
                top: eye_y as i32,
                right: (cx + eye_dx + er) as i32,
                bottom: (eye_y + er) as i32,
            };
            gdi::FillRect(hdc, &lid_rect, lid);
            gdi::DeleteObject(lid);
            paint_line(hdc, cx - eye_dx - er, eye_y, cx - eye_dx + er, eye_y, hog.outline, 2);
            paint_line(hdc, cx + eye_dx - er, eye_y, cx + eye_dx + er, eye_y, hog.outline, 2);
            paint_text(
                hdc,
                "z Z z",
                fnd::RECT {
                    left: (wf * 0.55) as i32,
                    top: (uf * 0.05) as i32,
                    right: (wf * 0.95) as i32,
                    bottom: (uf * 0.30) as i32,
                },
                0x008C8282,
                (uf * 0.16) as i32,
                true,
            );
        }
        Mood::Shocked => {
            // Giant pupils with tiny highlights (the mouth was drawn earlier).
            paint_ellipse(hdc, cx - eye_dx, eye_y, er * 0.85, er * 0.85, pupil, pupil);
            paint_ellipse(hdc, cx + eye_dx, eye_y, er * 0.85, er * 0.85, pupil, pupil);
            paint_ellipse(hdc, cx - eye_dx + er * 0.3, eye_y - er * 0.3, er * 0.25, er * 0.25, 0x00FFFFFF, 0x00FFFFFF);
            paint_ellipse(hdc, cx + eye_dx + er * 0.3, eye_y - er * 0.3, er * 0.25, er * 0.25, 0x00FFFFFF, 0x00FFFFFF);
        }
        Mood::Derpy => {
            // One regular eye, one enormous eye, tongue out.
            paint_ellipse(hdc, cx - eye_dx, eye_y, er * 0.5, er * 0.5, pupil, pupil);
            paint_ellipse(hdc, cx + eye_dx + er * 0.2, eye_y + er * 0.15, er * 1.4, er * 1.4, pupil, pupil);
            paint_ellipse(hdc, cx + rx * 0.10, snout_y + ry * 0.55, rx * 0.13, ry * 0.18, 0x008CA0E8, 0x008C78D6);
        }
    }
}
