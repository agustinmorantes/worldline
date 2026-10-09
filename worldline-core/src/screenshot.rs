//! A screenshot of a running game's window, through Windows.Graphics.Capture
//! (Windows 11). WGC copies the compositor's frame, so nothing touches the
//! game process. `Capturer::shot` takes one picture and reports how long each
//! stage took (for the `screenshot-stress` command); `shooter` takes them on
//! the checkpoint screenshot worker's thread.

pub use crate::detection::running::pids_under;
use crate::detection::worker::Shoot;
use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageBuffer, Rgba};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetThreadTimes, SetThreadPriority,
    THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClientRect, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, Interface, factory};

/// How long to wait for the first frame before giving up.
const TIMEOUT: Duration = Duration::from_millis(500);
const WIDTH: u32 = 1280;
const QUALITY: u8 = 80;

/// Wall time per stage of one shot, the thread's CPU time and the process's
/// memory afterwards.
#[derive(Debug, Default)]
pub struct Timings {
    pub window: Duration,
    /// Capture item, frame pool and session creation.
    pub session: Duration,
    /// Session start until the first frame arrived.
    pub first_frame: Duration,
    /// GPU downscale, copy to a staging texture, map and copy out.
    pub readback: Duration,
    pub resize: Duration,
    pub encode: Duration,
    pub total: Duration,
    pub cpu: Duration,
    pub source: (u32, u32),
    pub bytes: usize,
    pub working_set: usize,
    pub peak_working_set: usize,
}

pub enum Shot {
    /// The JPEG.
    Taken(Vec<u8>, Timings),
    /// Why no picture was taken.
    Skipped(&'static str, Timings),
}

/// Puts the calling thread in background mode (low CPU and I/O priority), once:
/// a second call fails with `ERROR_THREAD_MODE_ALREADY_BACKGROUND`.
pub fn background_thread() -> Result<()> {
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN)? };
    Ok(())
}

/// Our own D3D11 device, at low GPU priority. Use it from a thread put in
/// background mode by `background_thread`.
pub struct Capturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt_device: IDirect3DDevice,
}

impl Capturer {
    pub fn new() -> Result<Self> {
        unsafe {
            // Already initialized (in either mode) is fine for a free-threaded pool.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
            let device = device.context("no D3D11 device")?;
            let context = context.context("no D3D11 context")?;
            let dxgi: IDXGIDevice = device.cast()?;
            dxgi.SetGPUThreadPriority(-7)?;
            let winrt_device = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
            Ok(Self {
                device,
                context,
                winrt_device,
            })
        }
    }

    /// Takes one screenshot of the main window of the game running as `pids`,
    /// downscaled to `WIDTH` and encoded as JPEG.
    pub fn shot(&self, pids: &[u32]) -> Result<Shot> {
        let start = Instant::now();
        let cpu_start = thread_cpu();
        let mut t = Timings::default();
        let skipped = |reason, mut t: Timings| {
            t.total = start.elapsed();
            t.cpu = thread_cpu().saturating_sub(cpu_start);
            Ok(Shot::Skipped(reason, t))
        };

        if pids.is_empty() {
            return skipped("not running", t);
        }
        let stage = Instant::now();
        let window = main_window(pids);
        t.window = stage.elapsed();
        let Some(hwnd) = window else {
            return skipped("no window", t);
        };
        if unsafe { IsIconic(hwnd) }.as_bool() {
            return skipped("minimized", t);
        }

        let Some((bgra, width, height)) = self.grab(hwnd, &mut t)? else {
            return skipped("no frame", t);
        };

        let stage = Instant::now();
        let (bgra, width, height) = if width > WIDTH {
            let image = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bgra)
                .context("frame size mismatch")?;
            let new_height =
                ((u64::from(height) * u64::from(WIDTH) / u64::from(width)) as u32).max(1);
            let small = image::imageops::resize(&image, WIDTH, new_height, FilterType::Triangle);
            (small.into_raw(), WIDTH, new_height)
        } else {
            (bgra, width, height)
        };
        // BGRA to RGB: swap the channels and drop alpha.
        let rgb: Vec<u8> = bgra
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        t.resize = stage.elapsed();

        let stage = Instant::now();
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, QUALITY).encode(
            &rgb,
            width,
            height,
            ExtendedColorType::Rgb8,
        )?;
        t.encode = stage.elapsed();
        t.bytes = jpeg.len();

        t.total = start.elapsed();
        t.cpu = thread_cpu().saturating_sub(cpu_start);
        (t.working_set, t.peak_working_set) = memory();
        Ok(Shot::Taken(jpeg, t))
    }

    /// The window's next frame as tightly packed BGRA, or `None` if none
    /// arrived in time. The capture session lives only for this call.
    fn grab(&self, hwnd: HWND, t: &mut Timings) -> Result<Option<(Vec<u8>, u32, u32)>> {
        let stage = Instant::now();
        let item: GraphicsCaptureItem = unsafe {
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?.CreateForWindow(hwnd)?
        };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &self.winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            item.Size()?,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let result = (|| {
            session
                .SetIsBorderRequired(false)
                .context("can't hide the capture border (needs Windows 11)")?;
            session.SetIsCursorCaptureEnabled(false)?;
            let (arrived, arrival) = mpsc::sync_channel(1);
            pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
                let _ = arrived.try_send(());
                Ok(())
            }))?;
            t.session = stage.elapsed();
            let stage = Instant::now();
            session.StartCapture()?;
            let frame = match arrival.recv_timeout(TIMEOUT) {
                Ok(()) => pool.TryGetNextFrame().ok(),
                Err(_) => None,
            };
            t.first_frame = stage.elapsed();
            let Some(frame) = frame else {
                return Ok(None);
            };

            let stage = Instant::now();
            let content = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { texture.GetDesc(&mut desc) };
            let width = (content.Width as u32).min(desc.Width);
            let height = (content.Height as u32).min(desc.Height);
            t.source = (width, height);
            let pixels = self.read_halved(&texture, &desc, width, height)?;
            let _ = frame.Close();
            t.readback = stage.elapsed();
            Ok(Some(pixels))
        })();
        let _ = session.Close();
        let _ = pool.Close();
        result
    }

    /// Halves the frame on the GPU (mipmaps) while it stays at least `WIDTH`
    /// wide, then reads back that level; `shot` resizes the rest on the CPU.
    fn read_halved(
        &self,
        texture: &ID3D11Texture2D,
        desc: &D3D11_TEXTURE2D_DESC,
        width: u32,
        height: u32,
    ) -> Result<(Vec<u8>, u32, u32)> {
        let mut level = 0;
        while width >> (level + 1) >= WIDTH {
            level += 1;
        }
        if level == 0 {
            return self.read(texture, desc, 0, width, height);
        }
        let mips_desc = D3D11_TEXTURE2D_DESC {
            MipLevels: level + 1,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
            ArraySize: 1,
            ..*desc
        };
        unsafe {
            let mut mips = None;
            self.device
                .CreateTexture2D(&mips_desc, None, Some(&mut mips))?;
            let mips = mips.context("no mip texture")?;
            self.context
                .CopySubresourceRegion(&mips, 0, 0, 0, 0, texture, 0, None);
            let mut view = None;
            self.device
                .CreateShaderResourceView(&mips, None, Some(&mut view))?;
            self.context
                .GenerateMips(&view.context("no shader resource view")?);
            self.read(&mips, &mips_desc, level, width >> level, height >> level)
        }
    }

    /// Copies one mip level of `texture` to a staging texture and out to
    /// memory, cropped to `width` × `height`.
    fn read(
        &self,
        texture: &ID3D11Texture2D,
        desc: &D3D11_TEXTURE2D_DESC,
        level: u32,
        width: u32,
        height: u32,
    ) -> Result<(Vec<u8>, u32, u32)> {
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: (desc.Width >> level).max(1),
            Height: (desc.Height >> level).max(1),
            MipLevels: 1,
            ArraySize: 1,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..*desc
        };
        unsafe {
            let mut staging = None;
            self.device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            let staging = staging.context("no staging texture")?;
            self.context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, texture, level, None);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let row = width as usize * 4;
            let mut pixels = Vec::with_capacity(row * height as usize);
            for y in 0..height as usize {
                let src = (mapped.pData as *const u8).add(y * mapped.RowPitch as usize);
                pixels.extend_from_slice(std::slice::from_raw_parts(src, row));
            }
            self.context.Unmap(&staging, 0);
            Ok((pixels, width, height))
        }
    }
}

/// The largest visible top-level window owned by one of `pids`; a minimized
/// one counts, so the caller can skip it.
fn main_window(pids: &[u32]) -> Option<HWND> {
    struct Search<'a> {
        pids: &'a [u32],
        best: Option<(HWND, i64)>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> BOOL {
        let search = unsafe { &mut *(search.0 as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let candidate = search.pids.contains(&pid)
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
            && unsafe { GetWindow(hwnd, GW_OWNER) }.is_err()
            && unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW.0 == 0;
        if candidate {
            let area = if unsafe { IsIconic(hwnd) }.as_bool() {
                1
            } else {
                let mut rect = RECT::default();
                let _ = unsafe { GetClientRect(hwnd, &mut rect) };
                i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top)
            };
            if area > 0 && search.best.is_none_or(|(_, best)| area > best) {
                search.best = Some((hwnd, area));
            }
        }
        true.into()
    }
    let mut search = Search { pids, best: None };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.best.map(|(hwnd, _)| hwnd)
}

/// CPU time (kernel + user) of the calling thread.
fn thread_cpu() -> Duration {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    if unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .is_err()
    {
        return Duration::ZERO;
    }
    let ticks = |t: windows::Win32::Foundation::FILETIME| {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    };
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// The process's working set and peak working set, in bytes.
fn memory() -> (usize, usize) {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    match unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) } {
        Ok(()) => (counters.WorkingSetSize, counters.PeakWorkingSetSize),
        Err(_) => (0, 0),
    }
}

/// Whether screenshots can be taken here: Windows 11 (build 22000), the
/// first that captures a window without the yellow border.
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        windows_build() >= 22000
            && windows::Graphics::Capture::GraphicsCaptureSession::IsSupported().unwrap_or(false)
    })
}

fn windows_build() -> u32 {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|key| key.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|build| build.parse().ok())
        .unwrap_or(0)
}

/// Takes the checkpoint worker's shots. Called once, at the top of the
/// worker's thread, which it puts in background mode.
pub(crate) fn shooter() -> Shoot {
    if let Err(error) = background_thread() {
        eprintln!("Screenshot thread not in background mode: {error:#}");
    }
    // Created on the first shot. It is taken out while in use, so an error
    // or a panic drops it, in case the GPU was reset.
    let mut capturer: Option<Capturer> = None;
    Box::new(move |pids| {
        let current = match capturer.take() {
            Some(current) => current,
            None => Capturer::new()
                .inspect_err(|error| eprintln!("Screenshots unavailable: {error:#}"))
                .ok()?,
        };
        let shot = current.shot(pids);
        match shot {
            Ok(Shot::Taken(jpeg, _)) => {
                capturer = Some(current);
                Some(jpeg)
            }
            Ok(Shot::Skipped(..)) => {
                capturer = Some(current);
                None
            }
            Err(error) => {
                eprintln!("Screenshot failed: {error:#}");
                None
            }
        }
    })
}

/// The performance counter, to line shots up with PresentMon's `--qpc_time`.
pub fn qpc() -> i64 {
    let mut now = 0;
    let _ = unsafe { windows::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
    now
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a running app with a window: set `WORLDLINE_SHOT_DIR` to its
    /// install dir and run with `--ignored`.
    #[test]
    #[ignore]
    fn shoots_a_running_window() {
        assert!(supported());
        let dir = std::env::var("WORLDLINE_SHOT_DIR").unwrap();
        let pids = pids_under(&dir);
        let (tx, rx) = mpsc::channel();
        let worker = crate::detection::worker::Worker::start(
            shooter,
            Box::new(move |game_id, jpeg| {
                let _ = tx.send((game_id, jpeg.is_some()));
            }),
        );
        assert!(worker.request(7, pids.clone()));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), (7, true));
        // A second shot works too (background mode is set once per thread).
        assert!(worker.request(8, pids.clone()));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), (8, true));
        drop(worker);

        background_thread().unwrap();
        let capturer = Capturer::new().unwrap();
        let Shot::Taken(jpeg, t) = capturer.shot(&pids).unwrap() else {
            panic!("skipped");
        };
        let image = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(image.width(), t.source.0.min(WIDTH));
        println!("{t:?}");
        std::fs::write(std::env::temp_dir().join("wl-shot.jpg"), &jpeg).unwrap();
    }
}
