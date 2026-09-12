//! Media Foundation is loaded dynamically so Windows N can still open the UI.
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Media::MediaFoundation::*,
        System::{Com::*, LibraryLoader::*},
        UI::Shell::SHCreateMemStream,
    },
};
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
struct Library(HMODULE);
impl Library {
    unsafe fn new(name: PCWSTR) -> Result<Self> {
        Ok(Self(LoadLibraryExW(
            name,
            None,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )?))
    }
    unsafe fn symbol<T: Copy>(&self, name: PCSTR) -> Result<T> {
        let p = GetProcAddress(self.0, name).ok_or_else(Error::from_win32)?;
        Ok(std::mem::transmute_copy(&p))
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}
pub fn start(stop: Arc<AtomicBool>) -> Receiver<Frame> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return;
        }
        let _ = decode(&stop, |frame| tx.send(frame).is_ok());
        CoUninitialize();
    });
    rx
}
unsafe fn decode(stop: &AtomicBool, mut publish: impl FnMut(Frame) -> bool) -> Result<()> {
    let platform = Library::new(w!("mfplat.dll"))?;
    let readwrite = Library::new(w!("mfreadwrite.dll"))?;
    let startup: unsafe extern "system" fn(u32, u32) -> HRESULT =
        platform.symbol(s!("MFStartup"))?;
    let shutdown: unsafe extern "system" fn() -> HRESULT = platform.symbol(s!("MFShutdown"))?;
    let attributes: unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT =
        platform.symbol(s!("MFCreateAttributes"))?;
    let stream: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT =
        platform.symbol(s!("MFCreateMFByteStreamOnStream"))?;
    let media_type: unsafe extern "system" fn(*mut *mut c_void) -> HRESULT =
        platform.symbol(s!("MFCreateMediaType"))?;
    let source: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT =
        readwrite.symbol(s!("MFCreateSourceReaderFromByteStream"))?;
    startup(MF_VERSION, MFSTARTUP_FULL).ok()?;
    let result = (|| -> Result<()> {
        let mut raw = std::ptr::null_mut();
        attributes(&mut raw, 1).ok()?;
        let attr = IMFAttributes::from_raw(raw);
        attr.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let memory = SHCreateMemStream(Some(include_bytes!("../assets/hydra-clouds-2.mp4")))
            .ok_or_else(Error::from_win32)?;
        stream(memory.as_raw(), &mut raw).ok()?;
        let bytes = IMFByteStream::from_raw(raw);
        source(bytes.as_raw(), attr.as_raw(), &mut raw).ok()?;
        let reader = IMFSourceReader::from_raw(raw);
        let index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(index, true)?;
        media_type(&mut raw).ok()?;
        let requested = IMFMediaType::from_raw(raw);
        requested.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        requested.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(index, None, &requested)?;
        let actual = reader.GetCurrentMediaType(index)?;
        let size = actual.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (w, h) = ((size >> 32) as u32, size as u32);
        let stride = actual.GetUINT32(&MF_MT_DEFAULT_STRIDE).unwrap_or(w * 4) as i32;
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut clock = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            let mut flags = 0;
            let mut timestamp = 0;
            let mut sample = None;
            reader.ReadSample(
                index,
                0,
                None,
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                reader.SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(0i64))?;
                clock = Instant::now();
                continue;
            }
            if let Some(sample) = sample {
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut data = std::ptr::null_mut();
                let mut len = 0;
                buffer.Lock(&mut data, None, Some(&mut len))?;
                let mut pixels = vec![0u8; (w * h * 4) as usize];
                let valid = (stride.unsigned_abs() as u64 * h as u64) <= len as u64
                    && stride.unsigned_abs() >= w * 4;
                if valid {
                    for y in 0..h as usize {
                        let source_y = if stride < 0 { h as usize - 1 - y } else { y };
                        let row = std::slice::from_raw_parts(
                            data.add(source_y * stride.unsigned_abs() as usize),
                            w as usize * 4,
                        );
                        pixels[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(row);
                    }
                }
                buffer.Unlock()?;
                if !valid {
                    return Err(Error::from_hresult(E_FAIL));
                }
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    pixel[3] = 255;
                }
                let due = Duration::from_nanos(timestamp.max(0) as u64 * 100);
                while clock.elapsed() < due && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(
                        (due - clock.elapsed().min(due)).min(Duration::from_millis(10)),
                    );
                }
                let blocked = Instant::now();
                if !publish(Frame {
                    width: w,
                    height: h,
                    pixels,
                }) {
                    break;
                }
                if blocked.elapsed() > Duration::from_millis(100) {
                    clock += blocked.elapsed();
                }
            }
        }
        Ok(())
    })();
    let _ = shutdown();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires installed Media Foundation codecs"]
    fn decodes_embedded_video() {
        let stop = Arc::new(AtomicBool::new(false));
        let rx = start(stop.clone());
        let first = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("first video frame");
        let second = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second video frame");
        stop.store(true, Ordering::Relaxed);
        assert_eq!((first.width, first.height), (816, 464));
        assert_eq!(first.pixels.len(), 816 * 464 * 4);
        assert_ne!(first.pixels, second.pixels);
    }
}
