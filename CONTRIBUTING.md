# Contributing to Hydra Installer

Hydra Installer is a single native Windows x64 executable. Rust owns the window and
installer state; Direct2D/DirectWrite draw the interface on Direct3D 11, with WARP
software fallback. The executable uses Windows system libraries and requires no
separate runtime.

## Requirements

- Stable Rust with the `x86_64-pc-windows-msvc` target
- Visual Studio C++ Build Tools with the Windows SDK

GNU builds also work with a compatible MinGW toolchain.

## Building

```powershell
cargo build --release --target x86_64-pc-windows-msvc --locked
```

The output is `target/x86_64-pc-windows-msvc/release/hydra-installer.exe`, which can be
distributed directly. The MSVC CRT is linked statically.

CI builds every push and pull request, fails if the executable reaches 10,000,000
bytes, and publishes the executable to GitHub Releases for `v*` tags.

## Checks and tests

Run these before opening a pull request. CI runs the first three.

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Opt-in tests that need media components, the network or a launch fixture:

```powershell
cargo test decodes_embedded_video -- --ignored
cargo test live_release_lookup -- --ignored
rustc tests/fixtures/harmless.rs -o target/harmless.exe
$env:HYDRA_LAUNCH_FIXTURE = (Resolve-Path target/harmless.exe).Path
cargo test shell_fixtures -- --ignored
```

Default tests never touch Hydra data or launch installers. The shell test recycles
only its own disposable temporary directory and launches the harmless fixture. The
network test reads the public release endpoint.

## Debug flags

| Flag | Effect |
| --- | --- |
| `--no-video` | Use the still background instead of the video |
| `--software` | Force the WARP software renderer |
| `--lang <code>` | Override the Windows language (`en`, `pt`, `ru`, `es`) |
| `--preview <state>` | Render one state at a fixed animation time, with no networking |
| `--time <seconds>` | Animation time for `--preview` |
| `--capture <file.bmp>` | Save the previewed frame and exit |

`--preview` states: `download`, `checkbox`, `checked`, `dropdown`, `error` (Hydra
still running) or `error-<kind>` with a locale key, such as `error-offline`.

The README screenshot comes from:

```powershell
hydra-installer.exe --preview checkbox --lang en --time 4 --capture screenshot.bmp
```

## Project layout

| Path | Contents |
| --- | --- |
| `src/main.rs` | Window, input and startup |
| `src/render.rs` | Direct2D/DirectWrite rendering |
| `src/model.rs` | Installer state, release lookup and error kinds |
| `src/platform.rs` | Networking, downloads, cleanup and launching |
| `src/video.rs` | Media Foundation video decoding |
| `assets/` | Video, logo, icon, font and translations, all embedded in the executable |

## Adding a language

1. Copy `assets/locales/en.json` to `assets/locales/<code>.json` and translate the values.
   Keep placeholders such as `{progress}` unchanged.
2. Add the file to the `include_str!` list and the language code to the language list in
   `src/main.rs`.
3. Add the language's native name to `LANGUAGES` in `src/render.rs`, in the same order.
4. Check the result with `--preview dropdown --lang <code>` and a few `--preview error-<kind>`
   states, since longer translations can wrap.

## How it behaves

- The fixed window is 660 × 660 logical pixels and scales per monitor.
- Media Foundation decodes the video. Its DLLs load dynamically: if codecs or media
  components are absent, an embedded still with the same color conversion and blur
  is used instead.
- The language follows the Windows user language, with English fallback. A language
  picked in the menu lasts for the current session.
- Only release discovery and the installer download use the network, through WinHTTP
  and the system proxy and certificate store. The UI opens offline.
- Optional cleanup sends the `hydralauncher` folder in Roaming AppData to the Recycle
  Bin. It does not uninstall the existing Hydra application.
- Downloads use unique temporary directories and `.part` files. HTTP errors, truncated
  responses and write errors cannot trigger execution. A completed file is renamed
  before ShellExecute launches it.
- Failures come back as a category plus the raw message (`model::Kind`): Hydra still
  running (a file in the data folder is held open, so the shell aborts the recycle),
  cleanup failed, offline, unreachable, server error, bad release data, interrupted,
  disk full, file error, launch failed, or unknown. The error panel shows an icon, a
  plain-language explanation from `errors.<kind>` in the locales, and the raw message.
  The Install button becomes Try again, and cleanup failures add Install anyway, which
  unticks the cleanup option and starts.
- Closing during a download hides the window and cancels the worker. The process
  finishes cleanup when any pending WinHTTP operation returns (30-second I/O timeout).
  Successfully launched installers stay in their temporary folder for the child process.
- Tab and Shift+Tab navigate; Enter and Space activate; arrow keys select languages
  while the menu is open; Escape dismisses the menu. Dragging the background moves the
  window.

## Assets

The SVG and MP4 live in `assets/`. `logo.png` is a 2× rasterization of the SVG, and
`background.png` is the first decoded video frame. The renderer converts the SD
video's SMPTE-C primaries to match Chromium video compositing. Space Grotesk is
distributed under the included [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt) license.

UI changes ship in a new executable release.
