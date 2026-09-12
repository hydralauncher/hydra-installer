use serde::Deserialize;
use std::{
    fmt,
    io::{self, Read, Write},
};

pub const STATS_URL: &str = "https://hydra-api-us-east-1.losbroxas.org/stats";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    latest_release: Release,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn release(json: &[u8]) -> Result<(String, String), String> {
    let stats: Stats =
        serde_json::from_slice(json).map_err(|e| format!("Invalid release response: {e}"))?;
    let asset = stats
        .latest_release
        .assets
        .into_iter()
        .find(|a| a.name.ends_with("-setup.exe"))
        .ok_or("Setup executable not found in latest release assets")?;
    if !asset.browser_download_url.starts_with("https://") {
        return Err("Installer URL must use HTTPS".into());
    }
    Ok((
        stats.latest_release.tag_name.trim_start_matches('v').into(),
        asset.browser_download_url,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Ready,
    Downloading,
    Launching,
    Finished,
}
/// What went wrong, in terms the interface can explain. Each variant maps to a
/// locale entry under `errors`; the raw message travels alongside for support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    HydraRunning,
    CleanupFailed,
    Offline,
    Unreachable,
    ServerError,
    BadRelease,
    Interrupted,
    DiskFull,
    FileError,
    LaunchFailed,
    Unknown,
    Cancelled,
}
impl Kind {
    pub const ALL: [Kind; 12] = [
        Kind::HydraRunning,
        Kind::CleanupFailed,
        Kind::Offline,
        Kind::Unreachable,
        Kind::ServerError,
        Kind::BadRelease,
        Kind::Interrupted,
        Kind::DiskFull,
        Kind::FileError,
        Kind::LaunchFailed,
        Kind::Unknown,
        Kind::Cancelled,
    ];
    /// Locale key under `errors`, also the `--preview error-<key>` name.
    pub fn key(self) -> &'static str {
        match self {
            Kind::HydraRunning => "hydraRunning",
            Kind::CleanupFailed => "cleanupFailed",
            Kind::Offline => "offline",
            Kind::Unreachable => "unreachable",
            Kind::ServerError => "serverError",
            Kind::BadRelease => "badRelease",
            Kind::Interrupted => "interrupted",
            Kind::DiskFull => "diskFull",
            Kind::FileError => "fileError",
            Kind::LaunchFailed => "launchFailed",
            Kind::Unknown | Kind::Cancelled => "unknown",
        }
    }
    /// Failures of the optional cleanup step offer "Install anyway".
    pub fn cleanup(self) -> bool {
        matches!(self, Kind::HydraRunning | Kind::CleanupFailed)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: Kind,
    pub raw: String,
}
impl Failure {
    pub fn new(kind: Kind, raw: impl fmt::Display) -> Self {
        Self {
            kind,
            raw: raw.to_string(),
        }
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.raw)
    }
}
/// Windows reports a full volume as ERROR_DISK_FULL (112) or ERROR_HANDLE_DISK_FULL (39).
pub fn disk_kind(e: &io::Error) -> Kind {
    match e.raw_os_error() {
        Some(112) | Some(39) => Kind::DiskFull,
        _ => Kind::FileError,
    }
}
#[derive(Debug)]
pub enum Event {
    Version(String),
    Progress(u64, Option<u64>),
    Launching,
    Finished,
    Error(Failure),
}
pub struct Model {
    pub phase: Phase,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub version: Option<String>,
    pub error: Option<Failure>,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            phase: Phase::Ready,
            downloaded: 0,
            total: None,
            version: None,
            error: None,
        }
    }
}
impl Model {
    pub fn start(&mut self) -> bool {
        if self.phase != Phase::Ready {
            return false;
        }
        self.phase = Phase::Downloading;
        self.downloaded = 0;
        self.total = None;
        self.error = None;
        true
    }
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Version(v) => self.version = Some(v),
            Event::Progress(n, t) => {
                self.downloaded = n;
                self.total = t;
            }
            Event::Launching => self.phase = Phase::Launching,
            Event::Finished => self.phase = Phase::Finished,
            Event::Error(e) => {
                self.error = Some(e);
                self.phase = Phase::Ready;
            }
        }
    }
    pub fn percentage(&self) -> f32 {
        self.total.filter(|n| *n > 0).map_or(0., |n| {
            (self.downloaded as f64 / n as f64 * 100.).min(100.) as f32
        })
    }
}

/// A failed transfer, keeping which side failed so the interface can tell a
/// dropped connection from a full disk.
#[derive(Debug)]
pub enum CopyError {
    Read(io::Error),
    Write(io::Error),
    Oversized,
    Incomplete,
}
impl CopyError {
    pub fn kind(&self) -> Kind {
        match self {
            CopyError::Read(_) | CopyError::Oversized | CopyError::Incomplete => Kind::Interrupted,
            CopyError::Write(e) => disk_kind(e),
        }
    }
}
impl fmt::Display for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyError::Read(e) | CopyError::Write(e) => e.fmt(f),
            CopyError::Oversized => f.write_str("Download exceeds advertised size"),
            CopyError::Incomplete => f.write_str("Incomplete installer download"),
        }
    }
}
pub fn copy_download(
    mut reader: impl Read,
    mut writer: impl Write,
    total: Option<u64>,
    mut progress: impl FnMut(u64),
) -> Result<u64, CopyError> {
    let mut buffer = [0; 64 * 1024];
    let mut count = 0u64;
    loop {
        let n = reader.read(&mut buffer).map_err(CopyError::Read)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n]).map_err(CopyError::Write)?;
        count += n as u64;
        if total.is_some_and(|t| count > t) {
            return Err(CopyError::Oversized);
        }
        progress(count);
    }
    if count == 0 || total.is_some_and(|t| t != count) {
        return Err(CopyError::Incomplete);
    }
    writer.flush().map_err(CopyError::Write)?;
    Ok(count)
}

pub fn bytes(n: u64) -> String {
    let mut size = n as f64;
    let mut i = 0;
    while size >= 1024. && i < 4 {
        size /= 1024.;
        i += 1;
    }
    format!("{:.0} {}", size, ["B", "KB", "MB", "GB", "TB"][i])
}
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..16 {
        let u = (lo + hi) / 2.;
        let x = 3. * (1. - u) * (1. - u) * u * 0.4 + 3. * (1. - u) * u * u * 0.2 + u * u * u;
        if x < t {
            lo = u;
        } else {
            hi = u;
        }
    }
    let u = (lo + hi) / 2.;
    3. * (1. - u) * u * u + u * u * u
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_selection() {
        let (v,url)=release(br#"{"latestRelease":{"tagName":"v3.0","assets":[{"name":"x.zip","browserDownloadUrl":"https://a/x"},{"name":"hydra-setup.exe","browserDownloadUrl":"https://a/setup.exe"}]}}"#).unwrap();
        assert_eq!(v, "3.0");
        assert_eq!(url, "https://a/setup.exe");
        assert!(release(br#"{"latestRelease":{"tagName":"v3","assets":[]}}"#).is_err());
        assert!(release(b"bad json").is_err());
    }
    #[test]
    fn duplicate_and_retry() {
        let mut m = Model::default();
        assert!(m.start());
        assert!(!m.start());
        m.event(Event::Error(Failure::new(Kind::Offline, "offline")));
        assert_eq!(m.phase, Phase::Ready);
        assert!(m.start());
        assert!(m.error.is_none());
    }
    #[test]
    fn download_boundaries() {
        let data = b"installer";
        let mut output = Vec::new();
        assert_eq!(
            copy_download(&data[..], &mut output, None, |_| {}).unwrap(),
            9
        );
        assert_eq!(output, data);
        assert!(matches!(
            copy_download(&data[..], Vec::new(), Some(10), |_| {}),
            Err(CopyError::Incomplete)
        ));
        assert!(matches!(
            copy_download(&data[..], Vec::new(), Some(8), |_| {}),
            Err(CopyError::Oversized)
        ));
        assert!(matches!(
            copy_download(&b""[..], Vec::new(), None, |_| {}),
            Err(CopyError::Incomplete)
        ));
    }
    #[test]
    fn disk_and_network_failures() {
        struct Fail;
        impl Write for Fail {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from_raw_os_error(112))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl Read for Fail {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("disconnected"))
            }
        }
        let write = copy_download(&b"data"[..], Fail, None, |_| {}).unwrap_err();
        assert!(matches!(write, CopyError::Write(_)));
        assert_eq!(write.kind(), Kind::DiskFull);
        let read = copy_download(Fail, Vec::new(), None, |_| {}).unwrap_err();
        assert!(matches!(read, CopyError::Read(_)));
        assert_eq!(read.kind(), Kind::Interrupted);
        assert_eq!(disk_kind(&io::Error::from_raw_os_error(5)), Kind::FileError);
    }
    #[test]
    fn formatting() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "2 KB");
        assert!(ease(0.).abs() < 0.001);
        assert!((ease(1.) - 1.).abs() < 0.001);
    }
}
