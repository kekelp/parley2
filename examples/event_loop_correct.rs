// This example shows how to integrate the library with a smarter event loop in applications that pause their event loops when nothing is happening.
//
// See `event_loop_easy.rs` for a simpler way to integrate it.
//
// If you're building an application that never pauses its winit event loop, like a game, you can disregard all the wakeup mechanisms entirely. See the `basic.rs` example.

use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{EventLoop, ControlFlow},
    window::Window,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    
    event_loop
        .run_app(&mut Application { 
            state: None,
        })
        .unwrap();
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,

    text_renderer: TextRenderer,
    text: Text,

    _text_edit: TextEditHandle,
    _text_box: TextBoxHandle,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                required_features: Features::empty(),
                required_limits: Limits::default(),
                label: None,
                memory_hints: MemoryHints::default(),
            },
            None,
        ))
        .unwrap();

        let surface = instance.create_surface(window.clone()).unwrap();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode: PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        let text_renderer = TextRenderer::new(&device, &queue, surface_format);

        let mut text = Text::new_without_auto_wakeup();
        
        let text_edit = text.add_text_edit("This is a text edit box with a bunch of text that can be scrolled. Use the mouse wheel to get a smooth scroll animation. Focus this text box to see cursor blinking managed by the event loop timing.".to_string(), (50.0, 50.0), (400.0, 80.0), 0.0,);
        let text_box = text.add_text_box("This is a regular non-editable text box.", (50.0, 180.0), (500.0, 120.0), 0.0,);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text,
            _text_edit: text_edit,
            _text_box: text_box,
        }
    }

    fn render(&mut self) {
        println!("Rerender at {:?}", std::time::Instant::now());
        
        self.text.prepare_all(&mut self.text_renderer);
        self.text_renderer.gpu_load(&self.device, &self.queue);

        let surface_texture = self.surface.get_current_texture().unwrap();
        let view = surface_texture.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::RED),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            self.text_renderer.render(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
    }

    fn resize(&mut self, new_size: LogicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }
}

struct Application {
    state: Option<State>,
}

impl winit::application::ApplicationHandler<()> for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attributes = Window::default_attributes()
            .with_title("Manual cursor blink timing")
            .with_inner_size(LogicalSize::new(800, 600));
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        self.state = Some(State::new(window));
    }

    // These two methods integrate the cursor blinking wakeups in the "correct" winit way.
    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let state = self.state.as_mut().unwrap();

        // Check if we need to schedule a wake up for cursor blinking
        if let Some(blink_duration) = state.text.time_until_next_cursor_blink() {
            event_loop.set_control_flow(ControlFlow::wait_duration(blink_duration));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
    fn new_events(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, cause: winit::event::StartCause) {
        let state = self.state.as_mut().unwrap();

        if let winit::event::StartCause::ResumeTimeReached { .. } = cause {
            state.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        state.text.handle_event(&event, &state.window);

        match &event {
            WindowEvent::RedrawRequested => {
                state.render();
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(physical_size) => {
                let logical_size = LogicalSize::new(physical_size.width, physical_size.height);
                state.resize(logical_size);
            }
            _ => {}
        }

        if state.text.need_rerender() {
            state.window.request_redraw();
        }
    }
}