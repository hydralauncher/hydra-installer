# Hydra Installer

A single native Windows x64 executable. Rust owns the window and installer state;
Direct2D/DirectWrite draw the interface on Direct3D 11, with WARP software fallback.
The executable uses Windows system libraries and requires no separate runtime.

## Build

On Windows, install stable Rust and Visual Studio C++ Build Tools with the Windows SDK:

```powershell
cargo build --release --target x86_64-pc-windows-msvc --locked
```

Distribute `target/x86_64-pc-windows-msvc/release/hydra-installer.exe` directly.
The MSVC CRT is linked statically. CI enforces an executable smaller than 10,000,000
bytes and publishes the executable for `v*` tags. GNU builds also work with a
compatible MinGW toolchain; the locally validated GNU release is approximately 5.4 MB.

## Behavior

- The fixed window is 660 × 660 logical pixels and scales per monitor.
- Video, Space Grotesk, logo, icon and all four translations are embedded.
- Media Foundation decodes the video. Its DLLs load dynamically: absent codecs or
  media components use an embedded still with the same color conversion and blur.
- English, Portuguese, Russian and Spanish follow the Windows user language, with
  English fallback. The language choice lasts for the current session.
- Only release discovery and the installer download use the network, through
  WinHTTP and the system proxy/certificate store. The UI opens offline.
- Optional cleanup sends the `hydralauncher` folder in Roaming AppData to the
  Recycle Bin. It does not uninstall the existing Hydra application.
- Downloads use unique temporary directories and `.part` files. HTTP errors,
  truncated responses and write errors cannot trigger execution. A completed file
  is renamed before ShellExecute launches it.
- Failures come back as a category plus the raw message (`model::Kind`): Hydra still
  running (a file in the data folder is held open, so the shell aborts the recycle),
  cleanup failed, offline, unreachable, server error, bad release data, interrupted,
  disk full, file error, launch failed, or unknown. The description gives way to a
  panel with an icon, a plain-language explanation from `errors.<kind>` in the
  locales, and the raw message; the Install button becomes Try again, and cleanup
  failures add Install anyway, which unticks the cleanup option and starts.
- Closing during a download hides the window and cancels the worker. The process
  finishes cleanup when any pending WinHTTP operation returns (30-second I/O timeout).
  Successfully launched installers stay in their temporary folder for the child.
- Tab/Shift+Tab navigate; Enter/Space activate; arrows select languages with the
  menu open; Escape dismisses the menu. Background dragging moves the window.

## Verification

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo test decodes_embedded_video -- --ignored
cargo test live_release_lookup -- --ignored
rustc tests/fixtures/harmless.rs -o target/harmless.exe
$env:HYDRA_LAUNCH_FIXTURE = (Resolve-Path target/harmless.exe).Path
cargo test shell_fixtures -- --ignored
```

The opt-in shell test recycles only its own disposable temporary directory and
launches the harmless fixture. Default tests never touch Hydra data or launch
installers. The network test reads the public release endpoint.

`--no-video` forces the still in a normal run; `--software` forces WARP.
`--preview <state>` renders one state at a fixed animation time without networking:
`download`, `checkbox`, `checked`, `dropdown`, `error` (Hydra still running) or
`error-<kind>` with a locale key such as `error-offline`; add `--lang pt`,
`--time <seconds>` and `--capture out.bmp` to save the frame.

## Native assets

The SVG and MP4 live in `assets/`. `logo.png` is a 2× rasterization
of that SVG; `background.png` is the first decoded video frame. The renderer
converts the SD video's SMPTE-C primaries to match Chromium video compositing.
Space Grotesk is distributed under the included `assets/fonts/OFL.txt` license.

UI changes ship in a new executable release.
Rendering lives in `src/render.rs`, window/input code in `src/main.rs`,
and installer state, networking and media in the adjacent Rust modules.
