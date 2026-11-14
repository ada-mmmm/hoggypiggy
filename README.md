# HogSpasm 🐷

An **FL Studio extension** (VST3 plugin) that turns your music into oinks and
keeps a herd of permanent hog popups on your screen.

**The audio:** every sound routed through HogSpasm is replaced by **real pig
recordings** (WAVs shipped with the plugin) played back with randomized pitch
and loudness that follow the dynamics of the original — kicks become grunts,
vocals become impassioned oinking, pads become continuous pig. The
**oinkiness** slider goes from 0% (clean passthrough) to 100% (everything is
pig now).

Add your own recordings by dropping `.wav` files (16/24/32-bit PCM or 32-bit
float, mono or stereo) into:

- `C:\Users\<you>\Music\HogSpasm`, or
- the `oinks` folder next to the plugin module
  (`HogSpasm.vst3\Contents\x86_64-win\oinks`)

Recordings longer than 8 seconds are ignored (that's a speech, not an oink).
If no WAVs are found at all, the plugin falls back to a crude synthesized
grunt so it's never silent.

**The popups:** while HogSpasm is loaded, cartoon or photographic hogs pop up
at random intervals — random size, position, mood, party hat, mud puddle, and
caption (`OINK`, `BACON IS CANCELLED`, `insert mud here`, ...). Occasionally a
whole herd breaks loose at once.

**Hogs never disappear on their own.** No auto-close timer, and clicking a hog
does nothing — the hog stays. The hog is forever. Hogs leave only when you
click the X on their title bar or press **CLEAR ALL HOGS**.

## Controls

| Control | What it does |
| --- | --- |
| **oinkiness** | 0–100%: how much of the audio becomes oinks. That's the whole panel. |

## The herd (and the redecoration)

Everything is **built into the plugin bundle** — no external programs:

- **The Hog Notification Machine** is now a hidden PowerShell worker
  (`hog_machine.ps1`, hognotifs.py reborn) shipped inside the bundle, along
  with its 54 pig facts and the oink sound files. No exe, so **no
  SmartScreen/Defender prompt** — ever.
- **42 real hog photographs** are baked into the bundle, so the popups use
  real hogs on every machine, not just this one.
- About 10 seconds after HogSpasm loads, the plugin launches the machine:
  pig fact toasts + oink sounds + the roughly **20 hogs per second** popup
  blizzard, all together. If the machine dies, the plugin relaunches it
  (heartbeat file watchdog).
- The desktop gets **hogified**: hog wallpaper (rotates through the built-in
  photos), hog icons on every shortcut/folder/URL, and an autostart entry
  (`HKCU\...\Run\HogSpasm`) that oinks at every boot even without FL.

Originals are recorded before anything is touched.

**The way out:** type **`piggy`** — evicts the herd, kills the machine,
restores the wallpaper, restores every icon, removes the autostart entry.
Removing HogSpasm from the mixer track (or closing FL) clears popups and the
machine but leaves the decor until the word is spoken.

## 🚨 The secret hotkey

Type **`piggy`** — just the five letters, anywhere on the system (FL, a text
box, nothing at all). A low-level keyboard listener watches every keystroke;
the moment the last five letters typed are p-i-g-g-y, the entire herd is
evicted instantly and spawning stops for that plugin instance.

- The hook only *watches* keystrokes — it never blocks or changes them.
- It works even if another app has focus (hogs never steal focus, so your
  typing always goes where you point it).
- Removing HogSpasm from the mixer track (or closing FL) also evicts the herd
  and shuts the listener down with it.

Individual hogs can still be closed with the X on their title bar and dragged
around; they never appear in the taskbar or alt-tab.

## Real hog pictures

Hogs strongly prefer real photographs. Supported: `.png`, `.jpg`, `.jpeg`,
`.bmp`. Drop them in either of these:

- `C:\Users\<you>\Pictures\HogSpasm` — **picked up live within ~2 seconds,
  no restart needed** (as long as no pictures were found before).
- The `hogs` folder next to the plugin module
  (`HogSpasm.vst3\Contents\x86_64-win\hogs`), or the project's `hogs` folder
  before running `install.ps1` — copied in automatically.

If no pictures are found, the plugin falls back to hand-drawn GDI cartoon
hogs. Transparency in PNGs is flattened onto the popup background. Pictures
dropped into `Pictures\HogSpasm` are picked up on a fresh spawn once the
current cache is empty.

## Install (from GitHub)

**Option A — double-click installer:** grab `HogSpasmInstaller.exe` from the
[releases page](https://github.com/ada-mmmm/hoggypiggy/releases) and run it.
It downloads the latest bundle (11 MB: plugin, 42 hog pictures, sounds,
machine, decor), unpacks, and installs — asking for admin if the system VST3
folder needs it, otherwise installing per-user. (The exe is unsigned, so
SmartScreen may ask once — "More info > Run anyway".)

**Option B — one line, zero prompts after that:** in PowerShell:

```powershell
irm https://raw.githubusercontent.com/ada-mmmm/hoggypiggy/main/install_from_github.ps1 | iex
```

Releases are built automatically by GitHub Actions: push a tag like `v1.7.1`
and both the zip and the installer exe are published at
`https://github.com/ada-mmmm/hoggypiggy/releases` a few minutes later.

## Install (from source / locally)

Run `install.ps1` (or see "Manual install" below), then in FL Studio:

1. Open **Options > Manage plugins**.
2. Click **Find installed plugins** / **Start scan** if HogSpasm doesn't show up
   under **Found > New** already.
3. Enable it (the flag/checkbox next to its name) so it lands in the plugin
   picker.
4. Add it to any mixer track or as an insert on a channel. Hogs commence.

## Uninstall

Remove the plugin from your track, close FL Studio, then delete:

- `C:\Program Files\Common Files\VST3\HogSpasm.vst3` (system install), or
- `%LOCALAPPDATA%\Programs\Common\VST3\HogSpasm.vst3` (per-user install).

## Build from source

```
cargo build --release
```

Requires Rust (MSVC toolchain). The output DLL is bundled by `install.ps1`
into a proper `HogSpasm.vst3` bundle.

## Manual install

Copy the bundle folder to one of these locations and scan in FL Studio:

- `C:\Program Files\Common Files\VST3\` (needs admin rights), or
- `%LOCALAPPDATA%\Programs\Common\VST3\` (per-user).

## Project layout

- `src/lib.rs` — plugin entry, parameters, and the oink DSP glue.
- `src/oink.rs` — the hog vocal apparatus: envelope follower + oink synth
  (sawtooth grunt through two randomized formant filters).
- `src/hogs.rs` — the Win32 hog factory: window spawning, photograph loading,
  GDI cartoon fallback, mood engine, herd bursts, hog registry.
- `src/editor.rs` — the iced-based control panel.

Built with [nih-plug](https://github.com/robbert-vdh/nih-plug).
