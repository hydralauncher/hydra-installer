#![windows_subsystem = "windows"]
mod model;
mod platform;
mod render;
mod video;

use model::{Event, Failure, Kind, Model, Phase};
use render::{Layout, Renderer, View};
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::Instant,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Globalization::GetUserDefaultLocaleName,
        Graphics::Gdi::*,
        System::{Com::*, LibraryLoader::*},
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

struct App {
    renderer: Option<Renderer>,
    model: Model,
    strings: Vec<serde_json::Value>,
    language: usize,
    previous: bool,
    checked: bool,
    start: Instant,
    menu_open: bool,
    menu: f32,
    hover: (f32, f32),
    pressed: Option<usize>,
    focus: Option<usize>,
    layout: Layout,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    video: Option<Receiver<video::Frame>>,
    stop: Arc<AtomicBool>,
    font_path: PathBuf,
    software: bool,
    dpi: f32,
    progress: f32,
    active_step: f32,
    hover_amount: [f32; 8],
    check_amount: f32,
    error_amount: f32,
    last_tick: Instant,
    preview: bool,
    fixed_time: Option<f32>,
    capture: Option<PathBuf>,
    closing: bool,
}
impl App {
    fn new() -> std::result::Result<Self, String> {
        let strings = [
            include_str!("../assets/locales/en.json"),
            include_str!("../assets/locales/pt.json"),
            include_str!("../assets/locales/ru.json"),
            include_str!("../assets/locales/es.json"),
        ]
        .iter()
        .map(|s| serde_json::from_str(s).map_err(platform::err))
        .collect::<std::result::Result<Vec<_>, _>>()?;
        let args: Vec<_> = std::env::args().collect();
        let arg = |key: &str| {
            args.iter()
                .position(|a| a == key)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let preview = arg("--preview");
        // `--preview error` shows the Hydra-running panel; `--preview error-<key>` any other.
        let preview_error = preview.as_deref().and_then(|p| {
            let key = p.strip_prefix("error")?;
            let key = key.strip_prefix('-').unwrap_or("hydraRunning");
            Kind::ALL
                .into_iter()
                .find(|k| k.key() == key && *k != Kind::Cancelled)
        });
        let mut locale = [0u16; 85];
        unsafe {
            GetUserDefaultLocaleName(&mut locale);
        }
        let locale = arg("--lang").unwrap_or_else(|| String::from_utf16_lossy(&locale));
        let language = ["en", "pt", "ru", "es"]
            .iter()
            .position(|s| locale.to_lowercase().starts_with(s))
            .unwrap_or(0);
        let temp = platform::temporary_dir("assets").map_err(platform::err)?;
        let font_path = temp.join("SpaceGrotesk.ttf");
        std::fs::write(
            &font_path,
            include_bytes!("../assets/fonts/SpaceGrotesk.ttf"),
        )
        .map_err(platform::err)?;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut model = Model::default();
        if preview.is_some() {
            model.version = Some("3.0.0".into());
        }
        if preview.as_deref() == Some("download") {
            model.start();
            model.event(Event::Progress(45 * 1024 * 1024, Some(100 * 1024 * 1024)));
        }
        if let Some(kind) = preview_error {
            if kind == Kind::Interrupted {
                model.downloaded = 45 * 1024 * 1024;
                model.total = Some(100 * 1024 * 1024);
            }
            model.event(Event::Error(Failure::new(kind, sample_raw(kind))));
        }
        let cleanup_preview = preview_error.is_some_and(Kind::cleanup);
        let previous = if let Some(p) = preview.as_deref() {
            p == "checkbox" || p == "checked" || cleanup_preview
        } else {
            platform::data_dir().is_ok_and(|p| p.is_dir())
        };
        let now = Instant::now();
        let video = if args.iter().any(|s| s == "--no-video") || preview.is_some() {
            None
        } else {
            Some(video::start(stop.clone()))
        };
        Ok(Self {
            renderer: None,
            progress: model.percentage(),
            active_step: if model.phase == Phase::Ready { 0. } else { 1. },
            hover_amount: [0.; 8],
            check_amount: if preview.as_deref() == Some("checked") || cleanup_preview {
                1.
            } else {
                0.
            },
            error_amount: if preview_error.is_some() { 1. } else { 0. },
            model,
            strings,
            language,
            previous,
            checked: preview.as_deref() == Some("checked") || cleanup_preview,
            start: now,
            menu_open: preview.as_deref() == Some("dropdown"),
            menu: if preview.as_deref() == Some("dropdown") {
                1.
            } else {
                0.
            },
            hover: (-1., -1.),
            pressed: None,
            focus: None,
            layout: Layout::default(),
            tx,
            rx,
            video,
            stop,
            font_path,
            software: args.iter().any(|a| a == "--software"),
            dpi: arg("--scale")
                .and_then(|s| s.parse::<f32>().ok())
                .map_or(0., |s| 96. * s.clamp(1., 4.)),
            last_tick: now,
            preview: preview.is_some(),
            fixed_time: arg("--time")
                .and_then(|s| s.parse().ok())
                .or(preview.as_ref().map(|_| 4.)),
            capture: arg("--capture").map(PathBuf::from),
            closing: false,
        })
    }
    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        if self
            .fixed_time
            .unwrap_or_else(|| self.start.elapsed().as_secs_f32())
            < 2.5
        {
            return None;
        }
        if self.menu_open {
            for (i, r) in self.layout.options.iter().enumerate() {
                if r.contains(x, y) {
                    return Some(10 + i);
                }
            }
        }
        for (i, r) in [
            self.layout.language,
            self.layout.minimize,
            self.layout.close,
            self.layout.checkbox,
            self.layout.install,
            self.layout.secondary,
        ]
        .iter()
        .enumerate()
        {
            if r.contains(x, y) {
                return Some(i);
            }
        }
        None
    }
    fn activate(&mut self, id: usize) -> u8 {
        match id {
            0 => self.menu_open = !self.menu_open,
            1 => return 1,
            2 => return 2,
            3 => self.checked = !self.checked,
            // 4 retries as configured; 5 ("Install anyway") skips the cleanup that failed.
            4 | 5 => {
                if id == 5 {
                    self.checked = false;
                }
                if !self.preview && self.model.start() {
                    platform::download(self.tx.clone(), self.checked, self.stop.clone());
                }
            }
            10..=13 => {
                self.language = id - 10;
                self.menu_open = false;
            }
            _ => {}
        }
        0
    }
    unsafe fn draw(&mut self, hwnd: HWND) -> Result<()> {
        while let Ok(event) = self.rx.try_recv() {
            self.model.event(event);
        }
        let dt = self.last_tick.elapsed().as_secs_f32().min(0.1);
        self.last_tick = Instant::now();
        let target = if self.menu_open { 1. } else { 0. };
        self.menu += (target - self.menu).clamp(-dt / 0.2, dt / 0.2);
        self.progress += (self.model.percentage() - self.progress) * (dt / 0.3).min(1.);
        let step_target = if self.model.phase == Phase::Ready {
            0.
        } else {
            1.
        };
        self.active_step += (step_target - self.active_step).clamp(-dt / 0.4, dt / 0.4);
        let hover_rects = [
            self.layout.install,
            self.layout.minimize,
            self.layout.close,
            self.layout.options[0],
            self.layout.options[1],
            self.layout.options[2],
            self.layout.options[3],
            self.layout.secondary,
        ];
        for (i, rect) in hover_rects.iter().enumerate() {
            let target = if rect.contains(self.hover.0, self.hover.1) {
                1.
            } else {
                0.
            };
            self.hover_amount[i] += (target - self.hover_amount[i]).clamp(-dt / 0.25, dt / 0.25);
        }
        let target = if self.checked { 1. } else { 0. };
        self.check_amount += (target - self.check_amount).clamp(-dt / 0.2, dt / 0.2);
        let target = if self.model.error.is_some() { 1. } else { 0. };
        self.error_amount += (target - self.error_amount).clamp(-dt / 0.25, dt / 0.25);
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(
                hwnd,
                self.dpi,
                &self.font_path,
                self.software,
            )?);
        }
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| Error::from_hresult(E_FAIL))?;
        if let Some(rx) = &self.video {
            if let Ok(frame) = rx.try_recv() {
                renderer.frame(frame)?;
            }
        }
        self.layout = renderer.draw(View {
            model: &self.model,
            strings: &self.strings[self.language],
            language: self.language,
            previous: self.previous,
            checked: self.checked,
            elapsed: self
                .fixed_time
                .unwrap_or_else(|| self.start.elapsed().as_secs_f32()),
            menu: model::ease(self.menu),
            focus: self.focus,
            progress: self.progress,
            active_step: model::ease(self.active_step),
            hover_amount: self.hover_amount.map(model::ease),
            check_amount: model::ease(self.check_amount),
            error_amount: model::ease(self.error_amount),
        })?;
        if let Some(path) = self.capture.take() {
            renderer.capture(&path)?;
            PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0))?;
        }
        renderer.present()?;
        Ok(())
    }
}
/// Representative raw messages for `--preview error-<kind>` captures.
fn sample_raw(kind: Kind) -> &'static str {
    match kind {
        Kind::HydraRunning => {
            r"In use by another process: C:\Users\Ana\AppData\Roaming\hydralauncher\Local Storage\leveldb\LOCK"
        }
        Kind::CleanupFailed => "Access is denied. (0x80070005)",
        Kind::Offline => "The server name or address could not be resolved. (0x80072EE7)",
        Kind::Unreachable => "The operation timed out. (0x80072EE2)",
        Kind::ServerError => "Server returned HTTP 503",
        Kind::BadRelease => "Setup executable not found in latest release assets",
        Kind::Interrupted => "Incomplete installer download",
        Kind::DiskFull => "There is not enough space on the disk. (os error 112)",
        Kind::FileError => "Access is denied. (os error 5)",
        Kind::LaunchFailed => {
            "Failed to run installer: The operation was canceled by the user. (0x800704C7)"
        }
        Kind::Unknown | Kind::Cancelled => {
            r"The system cannot find the path specified. (os error 3) while creating C:\Users\Ana\AppData\Local\Temp\HydraInstaller-download-{5B1D…}"
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.renderer = None;
        let _ = std::fs::remove_file(&self.font_path);
        if let Some(dir) = self.font_path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

// Destroy the HWND before its App can be dropped, including initialization errors.
struct OwnedWindow(HWND);
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.0).as_bool() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let cs = &*(l.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    let mut action = 0u8;
    // Do not retain a mutable borrow when invoking APIs that synchronously reenter this procedure.
    let handled = if let Ok(mut app) = (*ptr).try_borrow_mut() {
        match message {
            WM_TIMER => {
                while let Ok(event) = app.rx.try_recv() {
                    app.model.event(event);
                }
                if app.model.phase == Phase::Finished
                    || (app.closing && app.model.phase == Phase::Ready)
                {
                    action = 2;
                }
                true
            }
            WM_CLOSE => {
                if app.model.phase == Phase::Downloading && !app.preview {
                    app.closing = true;
                    app.stop.store(true, Ordering::Relaxed);
                    action = 6;
                } else {
                    action = 2;
                }
                true
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut paint);
                // The swap-chain wait in the main loop schedules drawing. Paint
                // messages only validate damage, so input cannot add extra frames.
                let _ = EndPaint(hwnd, &paint);
                true
            }
            WM_DPICHANGED => {
                app.dpi = (w.0 & 0xffff) as f32;
                app.renderer = None;
                action = 3;
                true
            }
            WM_MOUSEMOVE => {
                let (x, y) = (
                    (l.0 as u16 as i16) as f32 * 96. / app.dpi,
                    ((l.0 >> 16) as u16 as i16) as f32 * 96. / app.dpi,
                );
                app.hover = (x, y);
                if app.layout.language.contains(x, y) {
                    app.menu_open = true;
                } else if !app.layout.dropdown.contains(x, y) && app.focus != Some(0) {
                    app.menu_open = false;
                }
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    ..Default::default()
                };
                let _ = TrackMouseEvent(&mut track);
                true
            }
            0x02A3 => {
                app.hover = (-1., -1.);
                if app.focus != Some(0) {
                    app.menu_open = false;
                }
                true
            }
            WM_LBUTTONDOWN => {
                let (x, y) = (
                    (l.0 as u16 as i16) as f32 * 96. / app.dpi,
                    ((l.0 >> 16) as u16 as i16) as f32 * 96. / app.dpi,
                );
                app.pressed = app.hit(x, y);
                app.focus = None;
                if app.pressed.is_none() {
                    app.menu_open = false;
                    action = 4;
                } else {
                    action = 5;
                }
                true
            }
            WM_LBUTTONUP => {
                let (x, y) = (
                    (l.0 as u16 as i16) as f32 * 96. / app.dpi,
                    ((l.0 >> 16) as u16 as i16) as f32 * 96. / app.dpi,
                );
                if let Some(id) = app.pressed.take() {
                    if app.hit(x, y) == Some(id) {
                        action = app.activate(id);
                    }
                }
                true
            }
            WM_KEYDOWN => {
                match w.0 as u16 {
                    9 => {
                        // language, minimize, close, checkbox, install, install anyway
                        let backwards = GetKeyState(VK_SHIFT.0 as i32) < 0;
                        let step = if backwards { 5 } else { 1 };
                        let ready = app.model.phase == Phase::Ready;
                        let available = |id: usize| match id {
                            3 => ready && app.previous,
                            4 => ready,
                            5 => {
                                ready && app.model.error.as_ref().is_some_and(|f| f.kind.cleanup())
                            }
                            _ => true,
                        };
                        let mut id = app
                            .focus
                            .map_or(if backwards { 5 } else { 0 }, |i| (i + step) % 6);
                        while !available(id) {
                            id = (id + step) % 6;
                        }
                        app.focus = Some(id);
                    }
                    13 | 32 => {
                        if let Some(id) = app.focus {
                            action = app.activate(id);
                        }
                    }
                    27 => {
                        app.menu_open = false;
                        app.focus = None;
                    }
                    38 | 40 if app.menu_open => {
                        let delta = if w.0 == 38 { 3 } else { 1 };
                        app.language = (app.language + delta) % 4;
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    } else {
        false
    };
    match action {
        1 => {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
        2 => {
            if message == WM_CLOSE || message == WM_TIMER {
                let _ = DestroyWindow(hwnd);
            } else {
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        3 => {
            let rect = &*(l.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        4 => {
            let _ = ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), l);
        }
        5 => {
            SetCapture(hwnd);
        }
        6 => {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        _ => {}
    }
    if message == WM_LBUTTONUP {
        let _ = ReleaseCapture();
    }
    if handled {
        LRESULT(0)
    } else {
        DefWindowProcW(hwnd, message, w, l)
    }
}
fn main() {
    if let Err(e) = unsafe { run() } {
        let message = platform::wide(&e);
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                w!("Hydra Installer"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}
unsafe fn run() -> std::result::Result<(), String> {
    CoInitializeEx(None, COINIT_APARTMENTTHREADED)
        .ok()
        .map_err(platform::err)?;
    let result = (|| -> std::result::Result<(), String> {
        let instance = GetModuleHandleW(None).map_err(platform::err)?;
        let class = w!("HydraNativeInstaller");
        // MAKEINTRESOURCEW(1): an integer resource identifier, never dereferenced.
        #[allow(clippy::manual_dangling_ptr)]
        let icon_id = PCWSTR(1usize as *const u16);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(platform::err)?,
            hIcon: LoadIconW(instance, icon_id).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(Error::from_win32().to_string());
        }
        let app = Box::new(RefCell::new(App::new()?));
        if app.borrow().dpi == 0. {
            app.borrow_mut().dpi = GetDpiForSystem() as f32;
        }
        let size = (660. * app.borrow().dpi / 96.).round() as i32;
        let mut work = RECT::default();
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .map_err(platform::err)?;
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            w!("Hydra Installer"),
            WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
            (work.left + work.right - size) / 2,
            (work.top + work.bottom - size) / 2,
            size,
            size,
            None,
            None,
            instance,
            Some((&*app as *const RefCell<App>).cast()),
        )
        .map_err(platform::err)?;
        let window = OwnedWindow(hwnd);
        {
            let mut a = app.borrow_mut();
            a.renderer =
                Some(Renderer::new(hwnd, a.dpi, &a.font_path, a.software).map_err(platform::err)?);
            a.start = Instant::now();
            a.last_tick = a.start;
            if !a.preview {
                let tx = a.tx.clone();
                std::thread::spawn(move || {
                    if let Ok((v, _)) = platform::latest() {
                        let _ = tx.send(Event::Version(v));
                    }
                });
            }
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        // Only poll worker completion here, including while minimized or closing.
        // Animation frames are paced by DXGI, not the coarse WM_TIMER clock.
        SetTimer(hwnd, 1, 100, None);
        let mut message = MSG::default();
        'running: loop {
            // Bound input processing so a busy queue cannot starve animation.
            for _ in 0..64 {
                if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                if message.message == WM_QUIT {
                    break 'running;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let visible = !app.borrow().closing && !IsIconic(hwnd).as_bool();
            if visible && app.borrow().renderer.is_none() {
                let mut a = app.borrow_mut();
                a.renderer = Renderer::new(hwnd, a.dpi, &a.font_path, a.software).ok();
            }
            let frame = if visible {
                app.borrow().renderer.as_ref().map(Renderer::frame_ready)
            } else {
                None
            };
            let handles: Vec<_> = frame.into_iter().collect();
            let ready =
                MsgWaitForMultipleObjectsEx(Some(&handles), 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            if ready == WAIT_FAILED {
                return Err(Error::from_win32().to_string());
            }
            if frame.is_some() && ready == WAIT_OBJECT_0 {
                let mut a = app.borrow_mut();
                if let Err(e) = a.draw(hwnd) {
                    a.renderer = None;
                    let _ = std::fs::write(
                        std::env::temp_dir().join("HydraInstaller-render-error.log"),
                        e.to_string(),
                    );
                }
            }
        }
        drop(window);
        drop(app);
        Ok(())
    })();
    CoUninitialize();
    result
}
