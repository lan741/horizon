//! The app: one window, one Metal surface, the touch model, and the frame clock.
//!
//! winit owns the UIKit lifecycle on iOS (`resumed` is `applicationDidBecomeActive`, `suspended`
//! is `applicationWillResignActive`), so this is also where the port answers rotation, the home
//! indicator, backgrounding and memory pressure. Every one of those is logged, because on a
//! device the log is the only channel: `Documents/Horizon/horizon-ios.log`.
use crate::gpu::{FrameInput, Gpu};
use crate::paths;
use crate::touch::Touches;
use std::io::Write;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// The app's entry: prepare storage, logging, then hand over to winit.
pub fn run() -> ! {
    // Before anything: a crash that kills the process leaves this file behind, so the last step
    // reached is known even when no console is attached (a sideloaded app has none).
    breadcrumb("main: entered");
    let data = match paths::prepare() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("Horizon iOS: cannot prepare the container: {error}");
            breadcrumb(&format!("main: container failed: {error}"));
            std::process::exit(2);
        }
    };
    breadcrumb(&format!("main: data root {}", data.display()));
    init_logging(&data);
    install_panic_hook();
    log::info!(
        "Horizon iOS {} starting: data {}, platform {} {}",
        env!("CARGO_PKG_VERSION"),
        data.display(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    breadcrumb("main: creating the event loop");
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("Horizon iOS: no event loop: {error}");
            breadcrumb(&format!("main: event loop failed: {error}"));
            std::process::exit(3);
        }
    };
    breadcrumb("main: event loop created, running");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    match event_loop.run_app(&mut app) {
        Ok(()) => {
            breadcrumb("main: event loop ended");
            std::process::exit(0)
        }
        Err(error) => {
            log::error!("Horizon iOS: the event loop stopped: {error}");
            breadcrumb(&format!("main: event loop stopped: {error}"));
            std::process::exit(4);
        }
    }
}

/// One line per startup step in `Documents/Horizon/boot.log`, flushed immediately: the file is the
/// only witness a crash leaves behind.
fn breadcrumb(step: &str) {
    use std::io::Write;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    let path = paths::data_dir().join("boot.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{:>7} ms  {step}", start.elapsed().as_millis());
        let _ = file.flush();
    }
}

/// Writing every line twice: to stderr (Console.app, `xcrun devicectl`) and to a file the Files
/// app can open, since a sideloaded app has no debugger attached.
fn init_logging(data: &std::path::Path) {
    static LOGGER: OnceLock<FileLogger> = OnceLock::new();
    let logger = LOGGER.get_or_init(|| FileLogger::new(data.join("horizon-ios.log")));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

struct FileLogger {
    file: std::sync::Mutex<Option<std::fs::File>>,
}

impl FileLogger {
    fn new(path: std::path::PathBuf) -> Self {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| eprintln!("Horizon iOS: log file {}: {error}", path.display()))
            .ok();
        Self {
            file: std::sync::Mutex::new(file),
        }
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("[{}] {}\n", record.level(), record.args());
        eprint!("{line}");
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {}
}

/// A crash on a device is a file, not a console: record where it happened before the process dies.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("Horizon iOS panic: {info}");
        breadcrumb(&format!("panic: {info}"));
        previous(info);
    }));
}

pub struct App {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    touches: Touches,
    started: Instant,
    /// The last frame count report: a 120 Hz panel must show ~120 here.
    window_started: Option<Instant>,
    frames_in_window: u32,
    /// Frames presented so far: the first one is a breadcrumb.
    frames_drawn: u64,
    /// Frames the surface refused, for throttled logging.
    failures: u64,
    reported_fps: f32,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            gpu: None,
            touches: Touches::default(),
            started: Instant::now(),
            window_started: None,
            frames_in_window: 0,
            frames_drawn: 0,
            failures: 0,
            reported_fps: 0.0,
        }
    }
}

impl App {
    /// Touch positions arrive in physical pixels; the model wants 0..=1 over the window.
    fn normalized(&self, location: winit::dpi::PhysicalPosition<f64>) -> Option<[f32; 2]> {
        let window = self.window.as_ref()?;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return None;
        }
        Some([
            (location.x as f32 / size.width as f32).clamp(0.0, 1.0),
            (location.y as f32 / size.height as f32).clamp(0.0, 1.0),
        ])
    }

    fn frame_input(&mut self) -> FrameInput {
        let look = self.touches.take_look();
        if self.touches.take_jump() {
            log::info!("Horizon iOS: jump (tap on the stick)");
        }
        let mut points = [[0.0f32; 3]; 2];
        for (slot, point) in self.touches.points().iter().take(2).enumerate() {
            points[slot] = [point.pos[0], point.pos[1], 1.0];
        }
        FrameInput {
            time: self.started.elapsed().as_secs_f32(),
            stick: self.touches.stick(),
            look,
            points,
        }
    }

    /// Report the measured frame rate every two seconds: the proof of 120 Hz on a ProMotion iPad.
    fn note_frame(&mut self) {
        let now = Instant::now();
        let started = *self.window_started.get_or_insert(now);
        self.frames_in_window += 1;
        let elapsed = now.duration_since(started);
        if elapsed >= Duration::from_secs(2) {
            let fps = self.frames_in_window as f32 / elapsed.as_secs_f32();
            self.frames_in_window = 0;
            self.window_started = Some(now);
            if (fps - self.reported_fps).abs() >= 1.0 {
                log::info!("Horizon iOS: {fps:.1} frames/s");
                self.reported_fps = fps;
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            // Returning from the background: the surface is stale, the window is not.
            if let (Some(gpu), Some(window)) = (self.gpu.as_mut(), self.window.as_ref()) {
                let size = window.inner_size();
                gpu.resize(size.width, size.height);
            }
            return;
        }
        let attributes = Window::default_attributes().with_title("Horizon");
        breadcrumb("resumed: creating the window");
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("Horizon iOS: no window: {error}");
                breadcrumb(&format!("resumed: no window: {error}"));
                event_loop.exit();
                return;
            }
        };
        breadcrumb("resumed: window created");
        log::info!(
            "Horizon iOS: window {}x{} at scale {}",
            window.inner_size().width,
            window.inner_size().height,
            window.scale_factor()
        );
        breadcrumb("resumed: opening the Metal surface");
        match Gpu::new(window.clone()) {
            Ok(gpu) => {
                breadcrumb("resumed: Metal surface open");
                self.gpu = Some(gpu)
            }
            Err(error) => {
                breadcrumb(&format!("resumed: no Metal surface: {error}"));
                // Keep the app alive: the log says why the screen stays dark, and the player can
                // still reach it from the Files app instead of facing a silent crash.
                log::error!("Horizon iOS: the Metal surface did not open: {error}");
            }
        }
        self.window = Some(window);
        self.started = Instant::now();
        self.window_started = None;
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        log::info!("Horizon iOS: suspended");
        self.touches.cancel();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Horizon iOS: close requested");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let now = Instant::now();
                match touch.phase {
                    TouchPhase::Started => {
                        if let Some(pos) = self.normalized(touch.location) {
                            self.touches.begin(touch.id, pos, now);
                        }
                    }
                    TouchPhase::Moved => {
                        if let Some(pos) = self.normalized(touch.location) {
                            self.touches.moved(touch.id, pos);
                        }
                    }
                    TouchPhase::Ended => {
                        if let Some(pos) = self.normalized(touch.location) {
                            self.touches.end(touch.id, pos, now);
                        }
                    }
                    TouchPhase::Cancelled => self.touches.cancel(),
                }
            }
            WindowEvent::Focused(false) => self.touches.cancel(),
            WindowEvent::RedrawRequested => {
                if self.frames_drawn == 0 {
                    breadcrumb("redraw: first frame");
                }
                self.frames_drawn = self.frames_drawn.saturating_add(1);
                let input = self.frame_input();
                if let Some(gpu) = self.gpu.as_mut()
                    && let Err(error) = gpu.render(&input)
                {
                    // A dropped frame is expected (a backgrounded screen); a running failure is
                    // not, and must not flood the log at the display's rate.
                    self.failures += 1;
                    if self.failures <= 3 || self.failures % 300 == 0 {
                        log::warn!(
                            "Horizon iOS: frame dropped ({} so far): {error}",
                            self.failures
                        );
                    }
                }
                self.note_frame();
                // One frame per display refresh: winit paces this from the CADisplayLink, so this
                // is 120 requests per second on a ProMotion panel and 60 elsewhere. Without a
                // device there is nothing to pace: wait for the system instead of spinning.
                if self.gpu.is_some()
                    && let Some(window) = self.window.as_ref()
                {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Only while frames can be drawn: without a device this would ask for 120 redraws a
        // second that render nothing.
        if self.gpu.is_some()
            && let Some(window) = self.window.as_ref()
        {
            window.request_redraw();
        }
    }
}
