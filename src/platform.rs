use crate::model::{self, Event, Failure, Kind};
use std::{
    ffi::c_void,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Networking::WinHttp::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
/// Classifies a WinHTTP failure: name resolution and connection refusals mean
/// the machine is offline; everything else is a reachable but failing path.
fn net(e: Error) -> Failure {
    let code = e.code().0 as u32;
    let win32 = if code >> 16 == 0x8007 {
        code & 0xffff
    } else {
        0
    };
    let kind = match win32 {
        12007 | 12029 => Kind::Offline,
        _ => Kind::Unreachable,
    };
    Failure::new(kind, e)
}
struct Internet(*mut c_void);
impl Internet {
    fn new(p: *mut c_void) -> Result<Self> {
        if p.is_null() {
            Err(Error::from_win32())
        } else {
            Ok(Self(p))
        }
    }
}
impl Drop for Internet {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}
pub struct Http {
    request: Internet,
    _connection: Internet,
    _session: Internet,
    pub total: Option<u64>,
}
impl Http {
    pub fn get(url: &str) -> std::result::Result<Self, Failure> {
        fn bad(e: impl std::fmt::Display) -> Failure {
            Failure::new(Kind::BadRelease, e)
        }
        unsafe {
            let url16 = wide(url);
            let mut parts = URL_COMPONENTS {
                dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
                dwHostNameLength: u32::MAX,
                dwUrlPathLength: u32::MAX,
                dwExtraInfoLength: u32::MAX,
                ..Default::default()
            };
            WinHttpCrackUrl(&url16[..url16.len() - 1], 0, &mut parts).map_err(bad)?;
            if parts.nScheme != WINHTTP_INTERNET_SCHEME_HTTPS {
                return Err(bad("Only HTTPS downloads are supported"));
            }
            if parts.dwHostNameLength == 0 || parts.lpszHostName.is_null() {
                return Err(bad("Download URL has no host"));
            }
            let host = wide(&String::from_utf16_lossy(std::slice::from_raw_parts(
                parts.lpszHostName.0,
                parts.dwHostNameLength as usize,
            )));
            let path = if parts.dwUrlPathLength == 0 {
                "/".into()
            } else {
                String::from_utf16_lossy(std::slice::from_raw_parts(
                    parts.lpszUrlPath.0,
                    parts.dwUrlPathLength as usize,
                ))
            };
            let extra = if parts.dwExtraInfoLength > 0 {
                String::from_utf16_lossy(std::slice::from_raw_parts(
                    parts.lpszExtraInfo.0,
                    parts.dwExtraInfoLength as usize,
                ))
            } else {
                String::new()
            };
            let path = wide(&format!("{path}{extra}"));
            let session = Internet::new(WinHttpOpen(
                w!("HydraInstaller/0.1"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ))
            .map_err(net)?;
            WinHttpSetTimeouts(session.0, 15000, 15000, 30000, 30000).map_err(net)?;
            let connection = Internet::new(WinHttpConnect(
                session.0,
                PCWSTR(host.as_ptr()),
                parts.nPort,
                0,
            ))
            .map_err(net)?;
            let request = Internet::new(WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ))
            .map_err(net)?;
            // Disallow HTTPS-to-HTTP redirects; use Windows' certificate validation unchanged.
            let policy = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
            WinHttpSetOption(
                Some(request.0),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&policy.to_le_bytes()),
            )
            .map_err(net)?;
            WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(net)?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(net)?;
            let mut status = 0u32;
            let mut len = 4;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&mut status as *mut u32).cast()),
                &mut len,
                std::ptr::null_mut(),
            )
            .map_err(net)?;
            if status != 200 {
                return Err(Failure::new(
                    Kind::ServerError,
                    format!("Server returned HTTP {status}"),
                ));
            }
            let mut value = [0u16; 32];
            let mut length = 64;
            let total = WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_CONTENT_LENGTH,
                PCWSTR::null(),
                Some(value.as_mut_ptr().cast()),
                &mut length,
                std::ptr::null_mut(),
            )
            .ok()
            .and_then(|_| {
                String::from_utf16_lossy(
                    &value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())],
                )
                .parse()
                .ok()
            });
            Ok(Self {
                request,
                _connection: connection,
                _session: session,
                total,
            })
        }
    }
}
impl Read for Http {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        unsafe {
            WinHttpReadData(
                self.request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len().min(u32::MAX as usize) as u32,
                &mut n,
            )
            .map_err(io::Error::other)?;
        }
        Ok(n as usize)
    }
}
pub fn latest() -> std::result::Result<(String, String), Failure> {
    let response = Http::get(model::STATS_URL)?;
    let mut data = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|e| Failure::new(Kind::Unreachable, e))?;
    if data.len() > 2 * 1024 * 1024 {
        return Err(Failure::new(Kind::BadRelease, "Release response too large"));
    }
    model::release(&data).map_err(|e| Failure::new(Kind::BadRelease, e))
}
pub fn data_dir() -> std::result::Result<PathBuf, String> {
    unsafe {
        let path =
            SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, HANDLE::default())
                .map_err(err)?;
        let result = path
            .to_string()
            .map(|p| PathBuf::from(p).join("hydralauncher"))
            .map_err(err);
        CoTaskMemFree(Some(path.0.cast()));
        result
    }
}
/// Finds a file another process holds open without delete sharing, which is
/// what stops the shell from recycling a folder while Hydra is running.
pub fn locked_file(dir: &Path) -> Option<PathBuf> {
    use std::os::windows::fs::OpenOptionsExt;
    const DELETE: u32 = 0x0001_0000;
    const SHARE_ALL: u32 = 7;
    let mut pending = vec![dir.to_path_buf()];
    let mut budget = 20_000;
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            budget -= 1;
            if budget == 0 {
                return None;
            }
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let probe = std::fs::OpenOptions::new()
                .access_mode(DELETE)
                .share_mode(SHARE_ALL)
                .open(&path);
            if probe
                .err()
                .is_some_and(|e| matches!(e.raw_os_error(), Some(32) | Some(33)))
            {
                return Some(path);
            }
        }
    }
    None
}
pub fn recycle(path: &Path) -> std::result::Result<(), Failure> {
    if !path.is_dir() {
        return Ok(());
    }
    fn err(e: impl std::fmt::Display) -> Failure {
        Failure::new(Kind::CleanupFailed, e)
    }
    unsafe {
        let op: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER).map_err(err)?;
        op.SetOperationFlags(
            FOF_ALLOWUNDO
                | FOF_NOCONFIRMATION
                | FOF_NOERRORUI
                | FOF_SILENT
                | FILEOPERATION_FLAGS(FOFX_RECYCLEONDELETE.0),
        )
        .map_err(err)?;
        let name = wide(&path.to_string_lossy());
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).map_err(err)?;
        op.DeleteItem(&item, None).map_err(err)?;
        op.PerformOperations().map_err(err)?;
        // With confirmations and error UI suppressed, an abort is never the
        // user's choice: some item could not be moved, usually because Hydra
        // still holds it open. Nothing is deleted in that case.
        if op.GetAnyOperationsAborted().map_err(err)?.as_bool() {
            return Err(match locked_file(path) {
                Some(file) => Failure::new(
                    Kind::HydraRunning,
                    format!("In use by another process: {}", file.display()),
                ),
                None => err("The shell cancelled the cleanup"),
            });
        }
        Ok(())
    }
}
pub fn launch(path: &Path) -> std::result::Result<(), Failure> {
    let path = wide(&path.to_string_lossy());
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
            lpVerb: w!("open"),
            lpFile: PCWSTR(path.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info).map_err(|e| {
            Failure::new(Kind::LaunchFailed, format!("Failed to run installer: {e}"))
        })?;
        if !info.hProcess.is_invalid() {
            let _ = CloseHandle(info.hProcess);
        }
        Ok(())
    }
}
pub fn download(tx: Sender<Event>, cleanup: bool, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let result = (|| -> std::result::Result<(), Failure> {
            let check_cancel = || {
                if cancel.load(Ordering::Relaxed) {
                    Err(Failure::new(Kind::Cancelled, "Download cancelled"))
                } else {
                    Ok(())
                }
            };
            check_cancel()?;
            if cleanup {
                recycle(&data_dir().map_err(|e| Failure::new(Kind::CleanupFailed, e))?)?;
            }
            check_cancel()?;
            let (version, url) = latest()?;
            let _ = tx.send(Event::Version(version));
            check_cancel()?;
            let response = Http::get(&url)?;
            let total = response.total;
            let dir =
                temporary_dir("download").map_err(|e| Failure::new(model::disk_kind(&e), e))?;
            let result = {
                let mut last = Instant::now() - Duration::from_secs(1);
                struct CancelRead {
                    inner: Http,
                    cancel: Arc<AtomicBool>,
                }
                impl Read for CancelRead {
                    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                        if self.cancel.load(Ordering::Relaxed) {
                            Err(io::Error::other("Download cancelled"))
                        } else {
                            self.inner.read(b)
                        }
                    }
                }
                download_to(
                    &dir,
                    CancelRead {
                        inner: response,
                        cancel: cancel.clone(),
                    },
                    total,
                    &cancel,
                    |n| {
                        if last.elapsed() >= Duration::from_millis(100) {
                            let _ = tx.send(Event::Progress(n, total));
                            last = Instant::now();
                        }
                    },
                    |path, n| {
                        let _ = tx.send(Event::Progress(n, Some(n)));
                        let _ = tx.send(Event::Launching);
                        launch(path)
                    },
                )
            };
            if result.is_err() {
                let _ = std::fs::remove_dir(&dir);
            }
            result?;
            // Leave the successfully launched executable available to the child process.
            std::thread::sleep(Duration::from_millis(500));
            let _ = tx.send(Event::Finished);
            Ok(())
        })();
        if let Err(e) = result {
            // A failure raised while the window is closing is just the cancellation.
            let e = if cancel.load(Ordering::Relaxed) {
                Failure::new(Kind::Cancelled, e.raw)
            } else {
                e
            };
            let _ = tx.send(Event::Error(e));
        }
        unsafe {
            CoUninitialize();
        }
    });
}
fn download_to(
    dir: &Path,
    reader: impl Read,
    total: Option<u64>,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
    launch: impl FnOnce(&Path, u64) -> std::result::Result<(), Failure>,
) -> std::result::Result<(), Failure> {
    let partial = dir.join("Hydra-setup.part");
    let complete = dir.join("Hydra-setup.exe");
    let disk = |e: io::Error| Failure::new(model::disk_kind(&e), e);
    let result = (|| -> std::result::Result<(), Failure> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(disk)?;
        let n = model::copy_download(reader, &mut file, total, progress)
            .map_err(|e| Failure::new(e.kind(), e))?;
        file.sync_all().map_err(disk)?;
        drop(file);
        if cancel.load(Ordering::Relaxed) {
            return Err(Failure::new(Kind::Cancelled, "Download cancelled"));
        }
        std::fs::rename(&partial, &complete).map_err(disk)?;
        launch(&complete, n)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_file(&complete);
    }
    result
}
pub fn temporary_dir(label: &str) -> io::Result<PathBuf> {
    let id = unsafe { CoCreateGuid().map_err(io::Error::other)? };
    let path = std::env::temp_dir().join(format!("HydraInstaller-{label}-{id:?}"));
    std::fs::create_dir(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staged_download_only_launches_complete_files() {
        let dir = temporary_dir("test").unwrap();
        let cancel = AtomicBool::new(false);
        let result = download_to(
            &dir,
            &b"fixture"[..],
            Some(7),
            &cancel,
            |_| {},
            |path, n| {
                assert_eq!(n, 7);
                assert_eq!(std::fs::read(path).unwrap(), b"fixture");
                assert!(!dir.join("Hydra-setup.part").exists());
                Ok(())
            },
        );
        assert!(result.is_ok());
        std::fs::remove_file(dir.join("Hydra-setup.exe")).unwrap();
        for total in [Some(8), Some(0)] {
            assert!(download_to(
                &dir,
                &b"fixture"[..],
                total,
                &cancel,
                |_| {},
                |_, _| panic!("must not launch incomplete data")
            )
            .is_err());
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        }
        let launch = download_to(
            &dir,
            &b"fixture"[..],
            None,
            &cancel,
            |_| {},
            |_, _| Err(Failure::new(Kind::LaunchFailed, "launch failed")),
        )
        .unwrap_err();
        assert_eq!(launch.kind, Kind::LaunchFailed);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        cancel.store(true, Ordering::Relaxed);
        let cancelled = download_to(
            &dir,
            &b"fixture"[..],
            None,
            &cancel,
            |_| {},
            |_, _| panic!("must not launch after cancellation"),
        )
        .unwrap_err();
        assert_eq!(cancelled.kind, Kind::Cancelled);
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn rejects_non_https() {
        let kind = Http::get("http://example.com/test").err().map(|e| e.kind);
        assert_eq!(kind, Some(Kind::BadRelease));
    }
    #[test]
    fn classifies_winhttp_codes() {
        let win32 = |n: u32| Error::from_hresult(HRESULT((0x8007_0000 | n) as i32));
        assert_eq!(net(win32(12007)).kind, Kind::Offline);
        assert_eq!(net(win32(12029)).kind, Kind::Offline);
        assert_eq!(net(win32(12002)).kind, Kind::Unreachable);
        assert_eq!(net(win32(12175)).kind, Kind::Unreachable);
        assert_eq!(net(Error::from_hresult(E_FAIL)).kind, Kind::Unreachable);
    }
    #[test]
    fn detects_files_held_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temporary_dir("locked").unwrap();
        std::fs::create_dir(dir.join("nested")).unwrap();
        std::fs::write(dir.join("free.txt"), b"x").unwrap();
        std::fs::write(dir.join("nested/held.txt"), b"y").unwrap();
        assert_eq!(locked_file(&dir), None);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(dir.join("nested/held.txt"))
            .unwrap();
        assert_eq!(locked_file(&dir), Some(dir.join("nested/held.txt")));
        drop(held);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore = "requires network"]
    fn live_release_lookup() {
        let (v, url) = latest().unwrap();
        assert!(!v.is_empty());
        assert!(url.starts_with("https://"));
    }
    #[test]
    #[ignore = "uses the Windows shell with disposable fixtures"]
    fn shell_fixtures() {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let dir = temporary_dir("recycle-test").unwrap();
        std::fs::write(dir.join("disposable.txt"), b"test fixture").unwrap();
        {
            use std::os::windows::fs::OpenOptionsExt;
            let held = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(dir.join("disposable.txt"))
                .unwrap();
            assert_eq!(recycle(&dir).unwrap_err().kind, Kind::HydraRunning);
            assert!(dir.join("disposable.txt").exists());
            drop(held);
        }
        recycle(&dir).unwrap();
        assert!(!dir.exists());
        let fixture = std::env::var_os("HYDRA_LAUNCH_FIXTURE")
            .expect("Set HYDRA_LAUNCH_FIXTURE to the compiled harmless fixture");
        launch(Path::new(&fixture)).unwrap();
        assert!(launch(&std::env::temp_dir().join("hydra-nonexistent-fixture.exe")).is_err());
        unsafe {
            CoUninitialize();
        }
    }
}
