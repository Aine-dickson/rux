//! Rux runtime shell — milestone M3.
//!
//! Opens a native window (winit), manages the GPU via vello's `RenderContext`,
//! loads a `.rux` document each frame's tree from `rux-runtime`, and paints it
//! (`rux-paint`). A `notify` file watcher wakes the event loop through an
//! `EventLoopProxy` on every save, so edits to the `.rux` file repaint live —
//! the hot-reload path from `docs/04-architecture.md`.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use notify::{EventKind, RecursiveMode, Watcher};
use rux_layout::{FocusRegion, HitRegion};
use rux_runtime::Document;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, RenderParams, Scene};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// Events delivered to the winit loop from outside it.
#[derive(Debug, Clone)]
enum RuxEvent {
    /// The `.rux` file changed on disk.
    Reload,
}

/// Taps closer than this (in physical pixels) between press and release still
/// count as a tap rather than a drag.
const TAP_SLOP: f64 = 6.0;

/// Rux screen background `#11111b`.
const BG: Color = Color::rgb8(0x11, 0x11, 0x1b);

/// Load a `.rux` document. On failure, log the diagnostic and fall back to an
/// empty screen so the window still opens (stand-in for the dev overlay).
fn load_document(path: &PathBuf) -> Document {
    match Document::load(path) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("failed to load {}: {err}", path.display());
            Document::from_source("<template><screen></screen></template>")
                .expect("empty document")
        }
    }
}

/// Per-window render state.
struct RenderState {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    scene: Scene,
}

/// The application: owns the vello render context, the document, the text
/// engine, input state, and (once resumed) one window.
struct App {
    context: RenderContext,
    state: Option<RenderState>,
    path: PathBuf,
    document: Document,
    text: rux_text::TextEngine,
    /// Hit regions from the most recent layout, for tap dispatch.
    hits: Vec<HitRegion>,
    /// Focusable input regions from the most recent layout.
    focuses: Vec<FocusRegion>,
    /// The `r-model` of the currently focused input, if any.
    focused: Option<String>,
    /// Current pointer position (physical pixels).
    pointer: (f64, f64),
    /// Where the left button was pressed, if it is currently down.
    press: Option<(f64, f64)>,
}

impl App {
    fn new(path: PathBuf) -> Self {
        let document = load_document(&path);
        Self {
            context: RenderContext::new(),
            state: None,
            path,
            document,
            text: rux_text::TextEngine::new(),
            hits: Vec::new(),
            focuses: Vec::new(),
            focused: None,
            pointer: (0.0, 0.0),
            press: None,
        }
    }

    /// Re-load the document after a file change. On a parse/load error we keep
    /// the last good document and log the diagnostic, rather than blanking the
    /// window (a first step toward the dev overlay).
    fn reload(&mut self) {
        match Document::load(&self.path) {
            Ok(doc) => {
                self.document = doc;
                eprintln!("reloaded {}", self.path.display());
            }
            Err(err) => eprintln!("reload failed for {}: {err}", self.path.display()),
        }
    }

    /// Handle a completed tap at `(px, py)`: focus an input if one is under the
    /// pointer, otherwise run the topmost `@tap` handler.
    fn dispatch_tap(&mut self, px: f64, py: f64) {
        // Focus takes precedence: an input under the pointer becomes focused.
        if let Some(model) = self
            .focuses
            .iter()
            .rev()
            .find(|f| f.contains(px as f32, py as f32))
            .map(|f| f.model.clone())
        {
            self.focused = Some(model);
            self.request_redraw();
            return;
        }
        // Tapping elsewhere drops focus.
        self.focused = None;

        // Topmost hit region wins (later in list = drawn on top).
        let handler = self
            .hits
            .iter()
            .rev()
            .find(|h| h.contains(px as f32, py as f32))
            .map(|h| h.on_tap.clone());

        if let Some(src) = handler {
            if self.document.engine_mut().run_handler(&src) {
                self.document.rebuild();
                self.request_redraw();
            }
        }
    }

    /// Apply a key to the focused input's bound signal, then rebuild + repaint.
    fn edit_focused(&mut self, key: &Key) {
        let Some(model) = self.focused.clone() else {
            return;
        };
        let mut value = self.document.engine_mut().get_string(&model);
        let changed = match key {
            Key::Named(NamedKey::Backspace) => value.pop().is_some(),
            Key::Named(NamedKey::Space) => {
                value.push(' ');
                true
            }
            Key::Character(s) => {
                let mut any = false;
                for c in s.chars().filter(|c| !c.is_control()) {
                    value.push(c);
                    any = true;
                }
                any
            }
            _ => false,
        };
        if changed {
            self.document.engine_mut().set_string(&model, &value);
            self.document.rebuild();
            self.request_redraw();
        }
    }

    fn request_redraw(&self) {
        if let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
    }

    fn render(&mut self) {
        // Split borrows so the text engine (used both to measure during layout
        // and to draw during paint) doesn't conflict with the render state.
        let App {
            context,
            state,
            document,
            text,
            hits,
            focuses,
            ..
        } = self;
        let Some(state) = state.as_mut() else {
            return;
        };
        let width = state.surface.config.width;
        let height = state.surface.config.height;

        // Layout (text sized via the engine's measure), then paint. Cache the
        // hit regions for tap dispatch.
        let layout = {
            let mut measure = |t: &str, fs: f32, w: u16, mw: Option<f32>| text.measure(t, fs, w, mw);
            rux_layout::layout(&document.root, width as f32, height as f32, &mut measure)
        };
        state.scene = rux_paint::build_scene(&layout.paints, text);
        *hits = layout.hits;
        *focuses = layout.focuses;

        let device_handle = &context.devices[state.surface.dev_id];
        let surface_texture = state
            .surface
            .surface
            .get_current_texture()
            .expect("get surface texture");

        state
            .renderer
            .render_to_surface(
                &device_handle.device,
                &device_handle.queue,
                &state.scene,
                &surface_texture,
                &RenderParams {
                    base_color: BG,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("render to surface");

        surface_texture.present();
    }
}

impl ApplicationHandler<RuxEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let title = format!(
            "Rux — {}",
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "M2".into())
        );
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(420.0, 640.0));
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));

        let size = window.inner_size();
        let surface = pollster::block_on(self.context.create_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("create surface");

        let device_handle = &self.context.devices[surface.dev_id];
        let renderer = Renderer::new(
            &device_handle.device,
            RendererOptions {
                surface_format: Some(surface.format),
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
            },
        )
        .expect("create renderer");

        self.state = Some(RenderState {
            window,
            surface,
            renderer,
            scene: Scene::new(),
        });
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: RuxEvent) {
        match event {
            RuxEvent::Reload => self.reload(),
        }
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_mut() {
                    self.context.resize_surface(
                        &mut state.surface,
                        size.width.max(1),
                        size.height.max(1),
                    );
                }
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = (position.x, position.y);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && self.focused.is_some() {
                    self.edit_focused(&event.logical_key);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.press = Some(self.pointer);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if let Some((sx, sy)) = self.press.take() {
                    let (px, py) = self.pointer;
                    if (px - sx).hypot(py - sy) <= TAP_SLOP {
                        self.dispatch_tap(px, py);
                    }
                }
            }
            // Event-driven: we only paint in response to a redraw request, which
            // is issued on resume, resize, reload, and tap — not every frame.
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }
}

/// Open the Rux window for the given `.rux` file and run the frame loop until the
/// window closes. Watches the file and repaints on change.
pub fn run(path: PathBuf) {
    let event_loop = EventLoop::<RuxEvent>::with_user_event()
        .build()
        .expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    // Watch the file's directory *recursively* so edits to imported components
    // (which live in subdirectories) also trigger a reload. Reload on any `.rux`
    // change — `Document::load` re-reads the main file and its components.
    let proxy = event_loop.create_proxy();
    let watch_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
            return;
        }
        let touches_rux = event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "rux"));
        if touches_rux {
            let _ = proxy.send_event(RuxEvent::Reload);
        }
    })
    .expect("create watcher");
    watcher
        .watch(&watch_dir, RecursiveMode::Recursive)
        .expect("watch directory");

    let mut app = App::new(path);
    event_loop.run_app(&mut app).expect("run app");

    drop(watcher); // keep the watcher alive for the loop's lifetime
}
