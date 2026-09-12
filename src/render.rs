use crate::{
    model::{self, Kind, Phase},
    platform::wide,
    video::Frame,
};
use std::path::Path;
use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::*,
            Direct3D11::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
            Imaging::*,
        },
        System::Com::*,
        UI::Shell::SHCreateMemStream,
    },
};

#[derive(Clone, Copy, Default, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    fn d2(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
}
#[derive(Default)]
pub struct Layout {
    pub install: Rect,
    pub checkbox: Rect,
    pub language: Rect,
    pub dropdown: Rect,
    pub options: [Rect; 4],
    pub close: Rect,
    pub minimize: Rect,
    pub secondary: Rect,
}
pub struct View<'a> {
    pub model: &'a model::Model,
    pub strings: &'a serde_json::Value,
    pub language: usize,
    pub previous: bool,
    pub checked: bool,
    pub elapsed: f32,
    pub menu: f32,
    pub focus: Option<usize>,
    pub progress: f32,
    pub active_step: f32,
    pub hover_amount: [f32; 8],
    pub check_amount: f32,
    pub error_amount: f32,
}
pub const LANGUAGES: [&str; 4] = ["English", "Português", "Русский", "Español"];
pub fn tr<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
/// Error copy lives under `errors.<kind>`; unknown kinds fall back to `unknown`.
fn tr_error<'a>(v: &'a serde_json::Value, kind: Kind, field: &str) -> &'a str {
    v["errors"][kind.key()][field]
        .as_str()
        .or_else(|| v["errors"]["unknown"][field].as_str())
        .unwrap_or("")
}

// Error icons: a 20 x 20 stroke grid, drawn with the same round-capped style.
#[derive(Clone, Copy)]
enum Seg {
    M(f32, f32),
    L(f32, f32),
    /// Clockwise arc of the given radius to a point.
    A(f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}
#[derive(Clone, Copy)]
enum Shape {
    Path(&'static [Seg]),
    Circle(f32, f32, f32),
    Rect(f32, f32, f32, f32, f32),
    Dot(f32, f32, f32),
}
use Seg::*;
fn icon_shapes(kind: Kind) -> &'static [Shape] {
    match kind {
        Kind::HydraRunning => &[
            Shape::Rect(2.5, 3.5, 15., 13., 2.),
            Shape::Path(&[M(2.5, 7.5), L(17.5, 7.5)]),
            Shape::Path(&[M(8., 11.), L(12., 15.)]),
            Shape::Path(&[M(12., 11.), L(8., 15.)]),
        ],
        Kind::CleanupFailed => &[
            Shape::Rect(4., 9., 12., 8.5, 2.),
            Shape::Path(&[
                M(6.75, 9.),
                L(6.75, 6.75),
                A(13.25, 6.75, 3.25),
                L(13.25, 9.),
            ]),
            Shape::Dot(10., 13.25, 0.9),
        ],
        Kind::Offline => &[
            Shape::Path(&[M(2.5, 7.5), A(8.5, 4.6, 11.)]),
            Shape::Path(&[M(13.6, 5.3), A(17.5, 7.5, 11.)]),
            Shape::Path(&[M(5.3, 10.6), A(8.7, 8.7, 7.)]),
            Shape::Path(&[M(12.9, 9.4), A(14.7, 10.6, 7.)]),
            Shape::Path(&[M(8., 13.6), A(12., 13.6, 3.)]),
            Shape::Path(&[M(3., 3.), L(17., 17.)]),
            Shape::Dot(10., 16.5, 0.8),
        ],
        Kind::Unreachable => &[
            Shape::Circle(10., 10., 7.25),
            Shape::Path(&[M(10., 5.75), L(10., 10.), L(13., 11.75)]),
        ],
        Kind::ServerError => &[
            Shape::Rect(3., 3.5, 14., 5.5, 1.5),
            Shape::Rect(3., 11., 14., 5.5, 1.5),
            Shape::Dot(6., 6.25, 0.8),
            Shape::Dot(6., 13.75, 0.8),
        ],
        Kind::BadRelease => &[
            Shape::Path(&[
                M(10., 2.75),
                L(17., 6.5),
                L(17., 13.5),
                L(10., 17.25),
                L(3., 13.5),
                L(3., 6.5),
                Z,
            ]),
            Shape::Path(&[M(3., 6.5), L(10., 10.25), L(17., 6.5)]),
            Shape::Path(&[M(10., 10.25), L(10., 17.25)]),
        ],
        Kind::Interrupted => &[
            Shape::Path(&[M(10., 3.), L(10., 12.)]),
            Shape::Path(&[M(6.5, 8.5), L(10., 12.), L(13.5, 8.5)]),
            Shape::Path(&[M(3.5, 15.5), L(7.5, 15.5)]),
            Shape::Path(&[M(12.5, 15.5), L(16.5, 15.5)]),
        ],
        Kind::DiskFull => &[
            Shape::Rect(2.5, 10.5, 15., 6.5, 1.5),
            Shape::Path(&[M(4., 10.5), L(6., 4.), L(14., 4.), L(16., 10.5)]),
            Shape::Dot(14., 13.75, 0.9),
        ],
        Kind::FileError => &[
            Shape::Path(&[
                M(4.75, 2.75),
                L(11.25, 2.75),
                L(15.5, 7.),
                L(15.5, 17.25),
                L(4.75, 17.25),
                Z,
            ]),
            Shape::Path(&[M(11.25, 2.75), L(11.25, 7.), L(15.5, 7.)]),
            Shape::Path(&[M(8., 10.5), L(12., 14.5)]),
            Shape::Path(&[M(12., 10.5), L(8., 14.5)]),
        ],
        Kind::LaunchFailed => &[
            Shape::Path(&[
                M(10., 2.75),
                L(16.25, 5.25),
                L(16.25, 9.75),
                C(16.25, 13.35, 13.65, 16.05, 10., 17.25),
                C(6.35, 16.05, 3.75, 13.35, 3.75, 9.75),
                L(3.75, 5.25),
                Z,
            ]),
            Shape::Path(&[M(10., 7.), L(10., 11.)]),
            Shape::Dot(10., 13.6, 0.8),
        ],
        Kind::Unknown | Kind::Cancelled => &[
            Shape::Path(&[M(10., 3.), L(17.5, 16.), L(2.5, 16.), Z]),
            Shape::Path(&[M(10., 8.), L(10., 12.)]),
            Shape::Dot(10., 14., 0.8),
        ],
    }
}

pub struct Renderer {
    frame_ready: HANDLE,
    dc: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    target: ID2D1Bitmap1,
    background: ID2D1Bitmap1,
    video: ID2D1Bitmap1,
    blur: ID2D1Effect,
    logo_blur: ID2D1Effect,
    glow: ID2D1Effect,
    dw: IDWriteFactory5,
    fonts: IDWriteFontCollection1,
    brush: ID2D1SolidColorBrush,
    factory: ID2D1Factory1,
    stroke: ID2D1StrokeStyle,
}
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.frame_ready);
        }
    }
}
impl Renderer {
    pub fn frame_ready(&self) -> HANDLE {
        self.frame_ready
    }
    pub unsafe fn present(&self) -> Result<()> {
        self.swap.Present(1, DXGI_PRESENT(0)).ok()
    }
    pub unsafe fn capture(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let size = self.target.GetPixelSize();
        let copy = self.dc.CreateBitmap(
            size,
            None,
            0,
            &D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.,
                dpiY: 96.,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            },
        )?;
        copy.CopyFromBitmap(None, &self.target, None)?;
        let mapped = copy.Map(D2D1_MAP_OPTIONS_READ)?;
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::File::create(path)?;
            let bytes = size.width * size.height * 4;
            file.write_all(b"BM")?;
            file.write_all(&(54 + bytes).to_le_bytes())?;
            file.write_all(&[0; 4])?;
            file.write_all(&54u32.to_le_bytes())?;
            file.write_all(&40u32.to_le_bytes())?;
            file.write_all(&size.width.to_le_bytes())?;
            file.write_all(&(-(size.height as i32)).to_le_bytes())?;
            file.write_all(&1u16.to_le_bytes())?;
            file.write_all(&32u16.to_le_bytes())?;
            file.write_all(&[0; 24])?;
            for y in 0..size.height {
                file.write_all(std::slice::from_raw_parts(
                    mapped.bits.add((y * mapped.pitch) as usize),
                    (size.width * 4) as usize,
                ))?;
            }
            Ok(())
        })();
        copy.Unmap()?;
        result.map_err(|e| Error::new(E_FAIL, e.to_string()))
    }
    pub unsafe fn new(hwnd: HWND, dpi: f32, font_path: &Path, software: bool) -> Result<Self> {
        let mut device = None;
        let hardware = if software {
            D3D_DRIVER_TYPE_WARP
        } else {
            D3D_DRIVER_TYPE_HARDWARE
        };
        if D3D11CreateDevice(
            None,
            hardware,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .is_err()
        {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
        }
        let device = device.ok_or_else(|| Error::from_hresult(E_FAIL))?;
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = dxgi.GetAdapter()?;
        let factory: IDXGIFactory2 = adapter.GetParent()?;
        let px = (660. * dpi / 96.).round() as u32;
        let swap = factory.CreateSwapChainForHwnd(
            &device,
            hwnd,
            &DXGI_SWAP_CHAIN_DESC1 {
                Width: px,
                Height: px,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
                ..Default::default()
            },
            None,
            None,
        )?;
        factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = factory
            .CreateDevice(&dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let surface: IDXGISurface = swap.GetBuffer(0)?;
        let target = dc.CreateBitmapFromDxgiSurface(
            &surface,
            Some(&D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: dpi,
                dpiY: dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            }),
        )?;
        dc.SetTarget(&target);
        dc.SetDpi(dpi, dpi);
        dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let background = dc.CreateBitmap(
            D2D_SIZE_U {
                width: 660,
                height: 660,
            },
            None,
            0,
            &D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.,
                dpiY: 96.,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                ..Default::default()
            },
        )?;
        let video = Self::png(&dc, include_bytes!("../assets/background.png"), 96.)?;
        let logo = Self::png(&dc, include_bytes!("../assets/logo.png"), 192.)?;
        // Chromium treats this untagged SD video as SMPTE-C/Rec.601. Convert its
        // decoded RGB primaries to sRGB in linear light, rather than displaying
        // the decoder's untagged RGB bytes as sRGB (which makes the teal too saturated).
        let linear = dc.CreateEffect(&CLSID_D2D1TableTransfer)?;
        linear.SetInput(0, &background, true);
        let mut decode = [0f32; 1024];
        let mut encode = [0f32; 1024];
        for i in 0..1024 {
            let x = i as f32 / 1023.;
            decode[i] = if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            };
            encode[i] = if x <= 0.0031308 {
                x * 12.92
            } else {
                1.055 * x.powf(1. / 2.4) - 0.055
            };
        }
        for property in [
            D2D1_TABLETRANSFER_PROP_RED_TABLE,
            D2D1_TABLETRANSFER_PROP_GREEN_TABLE,
            D2D1_TABLETRANSFER_PROP_BLUE_TABLE,
        ] {
            Self::property(&linear, property.0 as u32, &decode)?;
        }
        Self::property(
            &linear,
            D2D1_PROPERTY_PRECISION.0 as u32,
            &D2D1_BUFFER_PRECISION_16BPC_FLOAT.0,
        )?;
        let primaries = dc.CreateEffect(&CLSID_D2D1ColorMatrix)?;
        primaries.SetInput(0, &linear.GetOutput()?, true);
        Self::property(
            &primaries,
            D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32,
            &[
                0.9395f32, 0.0178, -0.0016, 0., 0.0502, 0.9658, -0.0044, 0., 0.0103, 0.0164, 1.006,
                0., 0., 0., 0., 1., 0., 0., 0., 0.,
            ],
        )?;
        Self::property(
            &primaries,
            D2D1_PROPERTY_PRECISION.0 as u32,
            &D2D1_BUFFER_PRECISION_16BPC_FLOAT.0,
        )?;
        let srgb = dc.CreateEffect(&CLSID_D2D1TableTransfer)?;
        srgb.SetInput(0, &primaries.GetOutput()?, true);
        for property in [
            D2D1_TABLETRANSFER_PROP_RED_TABLE,
            D2D1_TABLETRANSFER_PROP_GREEN_TABLE,
            D2D1_TABLETRANSFER_PROP_BLUE_TABLE,
        ] {
            Self::property(&srgb, property.0 as u32, &encode)?;
        }
        let blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur)?;
        blur.SetInput(0, &srgb.GetOutput()?, true);
        Self::property(
            &blur,
            D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
            &40f32,
        )?;
        Self::property(
            &blur,
            D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32,
            &D2D1_BORDER_MODE_HARD.0,
        )?;
        let compensation = dc.CreateEffect(&CLSID_D2D1DpiCompensation)?;
        compensation.SetInput(0, &logo, true);
        Self::property(
            &compensation,
            D2D1_DPICOMPENSATION_PROP_INPUT_DPI.0 as u32,
            &[192f32, 192.],
        )?;
        let logo_blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur)?;
        logo_blur.SetInput(0, &compensation.GetOutput()?, true);
        let glow = dc.CreateEffect(&CLSID_D2D1Shadow)?;
        glow.SetInput(0, &logo_blur.GetOutput()?, true);
        Self::property(
            &glow,
            D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32,
            &40f32,
        )?;
        Self::property(&glow, D2D1_SHADOW_PROP_COLOR.0 as u32, &[1f32, 1., 1., 0.5])?;
        let dw: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let path = wide(&font_path.to_string_lossy());
        let font = dw.CreateFontFileReference(PCWSTR(path.as_ptr()), None)?;
        let builder = dw.CreateFontSetBuilder()?;
        builder.AddFontFile(&font)?;
        let base: IDWriteFactory3 = dw.cast()?;
        let fonts = base.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?;
        let brush = dc.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 1.,
            },
            None,
        )?;
        let stroke = factory.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.,
                transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
            },
            None,
        )?;
        let swap2: IDXGISwapChain2 = swap.cast()?;
        swap2.SetMaximumFrameLatency(1)?;
        let frame_ready = swap2.GetFrameLatencyWaitableObject();
        if frame_ready.is_invalid() {
            return Err(Error::from_hresult(E_FAIL));
        }
        Ok(Self {
            frame_ready,
            dc,
            swap,
            target,
            background,
            video,
            blur,
            logo_blur,
            glow,
            dw,
            fonts,
            brush,
            factory,
            stroke: stroke.into(),
        })
    }
    unsafe fn icon(&self, kind: Kind, x: f32, y: f32, alpha: f32) -> Result<()> {
        self.color(1., 0.42, 0.42, alpha);
        let at = |px: f32, py: f32| D2D_POINT_2F {
            x: x + px,
            y: y + py,
        };
        for shape in icon_shapes(kind) {
            match *shape {
                Shape::Circle(cx, cy, r) => self.dc.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: at(cx, cy),
                        radiusX: r,
                        radiusY: r,
                    },
                    &self.brush,
                    1.6,
                    &self.stroke,
                ),
                Shape::Dot(cx, cy, r) => self.dc.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: at(cx, cy),
                        radiusX: r,
                        radiusY: r,
                    },
                    &self.brush,
                ),
                Shape::Rect(rx, ry, w, h, radius) => self.dc.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: Rect::new(x + rx, y + ry, w, h).d2(),
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &self.brush,
                    1.6,
                    &self.stroke,
                ),
                Shape::Path(segments) => {
                    let geometry = self.factory.CreatePathGeometry()?;
                    let sink = geometry.Open()?;
                    let mut open = false;
                    for segment in segments {
                        match *segment {
                            M(px, py) => {
                                if open {
                                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                                }
                                sink.BeginFigure(at(px, py), D2D1_FIGURE_BEGIN_HOLLOW);
                                open = true;
                            }
                            L(px, py) => sink.AddLine(at(px, py)),
                            A(px, py, r) => sink.AddArc(&D2D1_ARC_SEGMENT {
                                point: at(px, py),
                                size: D2D_SIZE_F {
                                    width: r,
                                    height: r,
                                },
                                rotationAngle: 0.,
                                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                arcSize: D2D1_ARC_SIZE_SMALL,
                            }),
                            C(x1, y1, x2, y2, px, py) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                                point1: at(x1, y1),
                                point2: at(x2, y2),
                                point3: at(px, py),
                            }),
                            Z => {
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                open = false;
                            }
                        }
                    }
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.Close()?;
                    self.dc
                        .DrawGeometry(&geometry, &self.brush, 1.6, &self.stroke);
                }
            }
        }
        Ok(())
    }
    unsafe fn property<T>(effect: &ID2D1Effect, index: u32, value: &T) -> Result<()> {
        effect.SetValue(
            index,
            D2D1_PROPERTY_TYPE_UNKNOWN,
            std::slice::from_raw_parts((value as *const T).cast(), std::mem::size_of::<T>()),
        )
    }
    unsafe fn png(dc: &ID2D1DeviceContext, bytes: &[u8], dpi: f32) -> Result<ID2D1Bitmap1> {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = SHCreateMemStream(Some(bytes)).ok_or_else(Error::from_win32)?;
        let frame = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)?
            .GetFrame(0)?;
        let convert = factory.CreateFormatConverter()?;
        convert.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.,
            WICBitmapPaletteTypeCustom,
        )?;
        dc.CreateBitmapFromWicBitmap(
            &convert,
            Some(&D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: dpi,
                dpiY: dpi,
                ..Default::default()
            }),
        )
    }
    pub unsafe fn frame(&mut self, frame: Frame) -> Result<()> {
        let size = self.video.GetPixelSize();
        if size.width == frame.width && size.height == frame.height {
            self.video
                .CopyFromMemory(None, frame.pixels.as_ptr().cast(), frame.width * 4)?;
        } else {
            self.video = self.dc.CreateBitmap(
                D2D_SIZE_U {
                    width: frame.width,
                    height: frame.height,
                },
                Some(frame.pixels.as_ptr().cast()),
                frame.width * 4,
                &D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.,
                    dpiY: 96.,
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }
    unsafe fn color(&self, r: f32, g: f32, b: f32, a: f32) {
        self.brush.SetColor(&D2D1_COLOR_F { r, g, b, a });
    }
    unsafe fn rect(&self, r: Rect, radius: f32, gray: f32, alpha: f32, border: Option<f32>) {
        self.color(gray, gray, gray, alpha);
        let shape = D2D1_ROUNDED_RECT {
            rect: r.d2(),
            radiusX: radius,
            radiusY: radius,
        };
        self.dc.FillRoundedRectangle(&shape, &self.brush);
        if let Some(a) = border {
            self.color(1., 1., 1., a);
            self.dc.DrawRoundedRectangle(&shape, &self.brush, 1., None);
        }
    }
    unsafe fn line(&self, x: f32, y: f32, x2: f32, y2: f32, alpha: f32, width: f32) {
        self.color(1., 1., 1., alpha);
        self.dc.DrawLine(
            D2D_POINT_2F { x, y },
            D2D_POINT_2F { x: x2, y: y2 },
            &self.brush,
            width,
            None,
        );
    }
    unsafe fn text_layout(
        &self,
        text: &str,
        size: f32,
        weight: i32,
        width: f32,
        center: bool,
    ) -> Result<IDWriteTextLayout> {
        let format = self.dw.CreateTextFormat(
            w!("Space Grotesk"),
            &self.fonts,
            DWRITE_FONT_WEIGHT(weight),
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-us"),
        )?;
        format.SetTextAlignment(if center {
            DWRITE_TEXT_ALIGNMENT_CENTER
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        })?;
        // Small print (the raw error line) sits on a 20px rhythm, everything else on 24px.
        let (line, baseline) = if size < 13. { (20., 15.) } else { (24., 18.) };
        format.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line, baseline)?;
        let text: Vec<u16> = text.encode_utf16().collect();
        self.dw.CreateTextLayout(&text, &format, width, 1000.)
    }
    unsafe fn measure(
        &self,
        text: &str,
        size: f32,
        weight: i32,
        width: f32,
    ) -> Result<DWRITE_TEXT_METRICS> {
        let l = self.text_layout(text, size, weight, width, false)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        l.GetMetrics(&mut m)?;
        Ok(m)
    }
    unsafe fn text(
        &self,
        text: &str,
        r: Rect,
        size: f32,
        weight: i32,
        alpha: f32,
        center: bool,
    ) -> Result<()> {
        let layout = self.text_layout(text, size, weight, r.w, center)?;
        self.color(1., 1., 1., alpha);
        self.dc.DrawTextLayout(
            D2D_POINT_2F { x: r.x, y: r.y },
            &layout,
            &self.brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        Ok(())
    }
    pub unsafe fn draw(&self, v: View<'_>) -> Result<Layout> {
        let dc = &self.dc;
        let opacity = model::ease((v.elapsed - 2.5) / 0.8);
        let mut layout = Layout::default();
        dc.SetTarget(&self.background);
        dc.SetDpi(96., 96.);
        dc.BeginDraw();
        dc.Clear(Some(&D2D1_COLOR_F {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 1.,
        }));
        let size = self.video.GetSize();
        let width = size.width * 660. / size.height;
        dc.DrawBitmap(
            &self.video,
            Some(&Rect::new((660. - width) / 2., 0., width, 660.).d2()),
            1.,
            D2D1_INTERPOLATION_MODE_LINEAR,
            None,
            None,
        );
        dc.EndDraw(None, None)?;
        dc.SetTarget(&self.target);
        let mut dpi = 0.;
        self.target.GetDpi(&mut dpi, &mut 0.);
        dc.SetDpi(dpi, dpi);
        dc.BeginDraw();
        dc.Clear(Some(&D2D1_COLOR_F {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 1.,
        }));
        dc.DrawImage(
            &self.blur.GetOutput()?,
            None,
            None,
            D2D1_INTERPOLATION_MODE_LINEAR,
            D2D1_COMPOSITE_MODE_SOURCE_OVER,
        );
        self.rect(Rect::new(0., 0., 660., 660.), 0., 0., 0.4, None);
        let desc = tr(v.strings, "description");
        let dh = self.measure(desc, 16., 400, 460.)?.height;
        let busy = v.model.phase != Phase::Ready;
        let ea = v.error_amount;
        // While an error is set, the panel takes the description's slot.
        const PANEL_TEXT: f32 = 460. - 32. - 20. - 12.;
        let panel = match &v.model.error {
            Some(f) => {
                let progress = match v.model.total {
                    Some(t) => tr(&v.strings["errors"], "progressOf")
                        .replace("{downloaded}", &model::bytes(v.model.downloaded))
                        .replace("{total}", &model::bytes(t)),
                    None => model::bytes(v.model.downloaded),
                };
                let body = tr_error(v.strings, f.kind, "body").replace("{progress}", &progress);
                let body_h = self.measure(&body, 14.4, 400, PANEL_TEXT)?.height;
                let raw_h = self.measure(&f.raw, 12.8, 400, PANEL_TEXT)?.height;
                Some((f, body, body_h, raw_h))
            }
            None => None,
        };
        let panel_h = panel.as_ref().map_or(dh, |(_, _, body_h, raw_h)| {
            14. + 24. + body_h + 6. + raw_h + 14.
        });
        let slot = dh + (panel_h - dh) * ea;
        let secondary = panel.as_ref().is_some_and(|(f, ..)| f.kind.cleanup());
        let header = 4. + 16. + 24. + if v.model.version.is_some() { 36. } else { 0. } + 16. + slot;
        let controls = if busy {
            46.
        } else {
            47. + if v.previous { 56. } else { 0. }
        };
        // Centred like every other state, but never up into the logo's glow.
        let top = ((660. - header - 32. - controls) / 2.).max(150.);
        let first = 16. - 12. * v.active_step;
        let second = 20. - first;
        self.rect(
            Rect::new(316., top, first, 4.),
            2.,
            1.,
            opacity * (1. - 0.5 * v.active_step),
            None,
        );
        self.rect(
            Rect::new(324. + first, top, second, 4.),
            2.,
            1.,
            opacity * (0.5 + 0.5 * v.active_step),
            None,
        );
        let title = v.strings["title"][if busy { "downloading" } else { "default" }]
            .as_str()
            .unwrap_or("Hydra Launcher");
        self.text(
            title,
            Rect::new(100., top + 25., 460., 24.),
            32.,
            600,
            opacity,
            true,
        )?;
        let mut y = top + 60.;
        if let Some(version) = &v.model.version {
            self.text(
                &format!("Ver. {version}"),
                Rect::new(100., top + 56., 460., 24.),
                13.6,
                400,
                opacity * 0.6,
                true,
            )?;
            y += 36.;
        }
        if ea < 0.999 {
            self.text(
                desc,
                Rect::new(100., y, 460., dh),
                16.,
                400,
                opacity * 0.8 * (1. - ea),
                true,
            )?;
        }
        if let Some((f, body, body_h, raw_h)) = &panel {
            let pa = opacity * ea;
            self.rect(
                Rect::new(100., y, 460., panel_h),
                8.,
                0.,
                0.3 * pa,
                Some(0.12 * pa),
            );
            self.icon(f.kind, 116., y + 16., pa)?;
            let x = 100. + 16. + 20. + 12.;
            self.text(
                tr_error(v.strings, f.kind, "title"),
                Rect::new(x, y + 14., PANEL_TEXT, 24.),
                15.2,
                500,
                pa,
                false,
            )?;
            self.text(
                body,
                Rect::new(x, y + 38., PANEL_TEXT, *body_h),
                14.4,
                400,
                pa * 0.7,
                false,
            )?;
            self.text(
                &f.raw,
                Rect::new(x, y + 44. + body_h, PANEL_TEXT, *raw_h),
                12.8,
                400,
                pa * 0.45,
                false,
            )?;
        }
        y += slot + 32.;
        if busy {
            self.rect(Rect::new(100., y, 460., 6.), 3., 1., opacity * 0.2, None);
            self.rect(
                Rect::new(100., y, 460. * v.progress / 100., 6.),
                3.,
                1.,
                opacity,
                None,
            );
            self.text(
                &format!("{:.0}%", v.model.percentage()),
                Rect::new(100., y + 22., 100., 24.),
                14.4,
                500,
                opacity,
                false,
            )?;
            let info = format!(
                "{} / {}",
                model::bytes(v.model.downloaded),
                v.model
                    .total
                    .map(model::bytes)
                    .unwrap_or_else(|| "...".into())
            );
            let tw = self.measure(&info, 14.4, 400, 460.)?.width;
            self.text(
                &info,
                Rect::new(560. - tw, y + 22., tw + 1., 24.),
                14.4,
                400,
                opacity * 0.9,
                false,
            )?;
        } else {
            if v.previous {
                let label = tr(v.strings, "deletePreviousInstallation");
                let tw = self.measure(label, 14.4, 400, 428.)?.width;
                layout.checkbox = Rect::new((660. - tw - 32.) / 2., y + 8., tw + 32., 24.);
                let x = layout.checkbox.x;
                self.rect(
                    Rect::new(x, y + 10., 20., 20.),
                    4.,
                    1.,
                    opacity * (0.1 + 0.1 * v.check_amount),
                    Some(opacity * (0.3 + 0.2 * v.check_amount)),
                );
                if v.checked {
                    self.line(x + 5., y + 20., x + 9., y + 24., opacity, 2.);
                    self.line(x + 9., y + 24., x + 15., y + 16., opacity, 2.);
                }
                self.text(
                    label,
                    Rect::new(x + 32., y + 8., 428., 24.),
                    14.4,
                    400,
                    opacity,
                    false,
                )?;
                y += 56.;
            }
            // The Install button becomes Try again while an error is shown; cleanup
            // failures add Install anyway beside it.
            let install = tr(v.strings, "startDownload");
            let retry = tr(v.strings, "tryAgain");
            let anyway = tr(v.strings, "installAnyway");
            let iw = self.measure(install, 16., 500, 460.)?.width + 48.;
            let rw = self.measure(retry, 16., 500, 460.)?.width + 48.;
            let pw = iw + (rw - iw) * ea;
            let sw = if secondary {
                self.measure(anyway, 16., 500, 460.)?.width + 48.
            } else {
                0.
            };
            let row = pw + if secondary { (12. + sw) * ea } else { 0. };
            let x = (660. - row) / 2.;
            layout.install = Rect::new(x, y, pw, 47.);
            self.rect(
                layout.install,
                6.,
                1.,
                opacity * (0.1 + 0.05 * v.hover_amount[0]),
                Some(opacity * (0.2 + 0.1 * v.hover_amount[0])),
            );
            let label = Rect::new(x, y + 11.5, pw, 24.);
            if ea < 0.999 {
                self.text(install, label, 16., 500, opacity * (1. - ea), true)?;
            }
            if ea > 0.001 {
                self.text(retry, label, 16., 500, opacity * ea, true)?;
            }
            if secondary {
                let sx = x + pw + 12. * ea;
                let sa = opacity * ea;
                layout.secondary = Rect::new(sx, y, sw, 47.);
                self.rect(
                    layout.secondary,
                    6.,
                    1.,
                    sa * 0.06 * v.hover_amount[7],
                    Some(sa * (0.2 + 0.1 * v.hover_amount[7])),
                );
                self.text(
                    anyway,
                    Rect::new(sx, y + 11.5, sw, 24.),
                    16.,
                    500,
                    sa * 0.8,
                    true,
                )?;
            }
        }
        // Logo transitions use the same cubic-bezier(0.4,0,0.2,1) as the web UI.
        let focus = model::ease((v.elapsed - 0.1) / 1.5);
        let shrink = model::ease((v.elapsed - 1.6) / 1.2);
        let scale = 1. - 0.65 * shrink;
        Self::property(
            &self.logo_blur,
            D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
            &(20. * (1. - focus)),
        )?;
        let center_y = 330. - 242. * shrink;
        dc.SetTransform(&Matrix3x2 {
            M11: scale,
            M12: 0.,
            M21: 0.,
            M22: scale,
            M31: 330. - 112. * scale,
            M32: center_y - 108. * scale,
        });
        // A layer controls opacity for the logo and its shadow together.
        dc.PushLayer(
            &D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F {
                    left: -100.,
                    top: -100.,
                    right: 324.,
                    bottom: 316.,
                },
                opacity: focus,
                maskTransform: Matrix3x2::identity(),
                ..Default::default()
            },
            None,
        );
        dc.DrawImage(
            &self.glow.GetOutput()?,
            None,
            None,
            D2D1_INTERPOLATION_MODE_LINEAR,
            D2D1_COMPOSITE_MODE_SOURCE_OVER,
        );
        dc.DrawImage(
            &self.logo_blur.GetOutput()?,
            None,
            None,
            D2D1_INTERPOLATION_MODE_LINEAR,
            D2D1_COMPOSITE_MODE_SOURCE_OVER,
        );
        dc.PopLayer();
        dc.SetTransform(&Matrix3x2::identity());
        let name = LANGUAGES[v.language];
        let nw = self.measure(name, 14., 500, 200.)?.width;
        layout.language = Rect::new(20., 20., 18. + 8. + nw + 8. + 16., 24.);
        self.color(1., 1., 1., opacity);
        let globe = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: 29., y: 29. },
            radiusX: 6.6,
            radiusY: 6.6,
        };
        dc.DrawEllipse(&globe, &self.brush, 1.3, None);
        dc.DrawEllipse(
            &D2D1_ELLIPSE {
                radiusX: 3.,
                ..globe
            },
            &self.brush,
            1.,
            None,
        );
        self.line(22.5, 29., 35.5, 29., opacity, 1.);
        self.text(
            name,
            Rect::new(46., 17., nw + 1., 24.),
            14.,
            500,
            opacity,
            false,
        )?;
        let ax = 54. + nw;
        let angle = std::f32::consts::PI * v.menu;
        let (c, s) = (angle.cos(), angle.sin());
        let rotate = |x: f32, y: f32| (ax + 8. + x * c - y * s, 29. + x * s + y * c);
        let (a, b) = rotate(-4., -2.);
        let (c, d) = rotate(0., 2.);
        let (e, f) = rotate(4., -2.);
        self.line(a, b, c, d, opacity, 1.2);
        self.line(c, d, e, f, opacity, 1.2);
        layout.minimize = Rect::new(564., 20., 30., 30.);
        layout.close = Rect::new(610., 20., 30., 30.);
        for (i, r) in [layout.minimize, layout.close].into_iter().enumerate() {
            self.rect(r, 4., 1., opacity * 0.1 * v.hover_amount[i + 1], None);
        }
        self.line(573., 35., 585., 35., opacity * 0.8, 1.2);
        self.line(621., 31., 629., 39., opacity * 0.8, 1.2);
        self.line(629., 31., 621., 39., opacity * 0.8, 1.2);
        layout.dropdown = Rect::new(20., 42., 180., 182.);
        if v.menu > 0.001 {
            let ma = opacity * v.menu;
            let dy = -10. * (1. - v.menu);
            let sc = 0.95 + 0.05 * v.menu;
            dc.SetTransform(&Matrix3x2 {
                M11: sc,
                M22: sc,
                M31: 110. * (1. - sc),
                M32: 133. * (1. - sc) + dy,
                ..Matrix3x2::identity()
            });
            self.rect(layout.dropdown, 8., 0., ma * 0.6, Some(ma * 0.2));
            for (i, label) in LANGUAGES.iter().enumerate() {
                let r = Rect::new(29., 51. + i as f32 * 42., 162., 38.);
                layout.options[i] = r;
                if i == v.language || v.hover_amount[i + 3] > 0. {
                    self.rect(
                        r,
                        0.,
                        1.,
                        ma * if i == v.language {
                            0.15
                        } else {
                            0.1 * v.hover_amount[i + 3]
                        },
                        None,
                    );
                }
                self.text(
                    label,
                    Rect::new(41., r.y + 8., 140., 24.),
                    14.,
                    if i == v.language { 600 } else { 500 },
                    ma,
                    false,
                )?;
            }
            dc.SetTransform(&Matrix3x2::identity());
        }
        if let Some(i) = v.focus {
            let r = match i {
                0 => layout.language,
                1 => layout.minimize,
                2 => layout.close,
                3 if v.previous => layout.checkbox,
                5 => layout.secondary,
                _ => layout.install,
            };
            self.color(1., 1., 1., opacity * 0.6);
            dc.DrawRectangle(&r.d2(), &self.brush, 1., None);
        }
        dc.EndDraw(None, None)?;
        Ok(layout)
    }
}
