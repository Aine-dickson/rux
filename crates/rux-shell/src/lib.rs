//! Rux runtime shell — milestone M1.
//!
//! Opens a native window (winit), manages the GPU via vello's `RenderContext`,
//! and every frame lays out a hand-built node tree (`rux-layout`) and paints it
//! as a vello scene (`rux-paint`). This proves the layout → paint → present
//! pipeline end to end. The demo tree stands in until the parser (M2) feeds
//! real `.rux` documents.

use std::num::NonZeroUsize;
use std::sync::Arc;

use rux_layout::{Axis, Node, Rgba, Style};
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, RenderParams, Scene};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Rux screen background `#11111b`.
const BG: Color = Color::rgb8(0x11, 0x11, 0x1b);

/// Build the M1 demo tree: a screen containing a rounded card with two rows.
/// Roughly the guide's battery card, expressed directly in `rux-layout` terms.
fn demo_tree() -> Node {
    let card = Node::new(Style {
        grow: 0.0,
        width: Some(320.0),
        padding: 16.0,
        gap: 12.0,
        axis: Axis::Column,
        background: Some(Rgba::new(0.118, 0.118, 0.180, 1.0)), // #1e1e2e
        radius: 12.0,
        ..Default::default()
    })
    .with(Node::new(Style {
        height: Some(28.0),
        background: Some(Rgba::new(0.651, 0.890, 0.631, 1.0)), // #a6e3a1
        radius: 6.0,
        ..Default::default()
    }))
    .with(Node::new(Style {
        height: Some(16.0),
        width: Some(180.0),
        background: Some(Rgba::new(0.576, 0.600, 0.729, 1.0)), // #9399b2
        radius: 6.0,
        ..Default::default()
    }));

    // The screen: fills the window, centres nothing yet (M1 is top-left flow),
    // just pads and holds the card.
    Node::new(Style {
        grow: 1.0,
        padding: 24.0,
        gap: 0.0,
        axis: Axis::Column,
        background: None, // the window clear colour is the screen background
        ..Default::default()
    })
    .with(card)
}

/// Per-window render state.
struct RenderState {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    scene: Scene,
}

/// The application: owns the vello render context and (once resumed) one window.
struct App {
    context: RenderContext,
    state: Option<RenderState>,
    tree: Node,
}

impl App {
    fn new() -> Self {
        Self {
            context: RenderContext::new(),
            state: None,
            tree: demo_tree(),
        }
    }

    fn render(&mut self) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let width = state.surface.config.width;
        let height = state.surface.config.height;

        // Layout the tree into the current viewport, then paint it.
        let rects = rux_layout::layout(&self.tree, width as f32, height as f32);
        state.scene = rux_paint::build_scene(&rects);

        let device_handle = &self.context.devices[state.surface.dev_id];
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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Rux — M1")
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
            }
            WindowEvent::RedrawRequested => {
                self.render();
                if let Some(state) = self.state.as_ref() {
                    state.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Open the Rux window and run the frame loop until the window closes.
pub fn run() {
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run app");
}
