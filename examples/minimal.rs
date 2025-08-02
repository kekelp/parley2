use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use winit::{dpi::LogicalSize, event::WindowEvent, event_loop::EventLoop, window::Window};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut Application { state: None }).unwrap();
}

struct State {
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,
    text: Text,
    text_renderer: TextRenderer,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let surface = instance.create_surface(window.clone()).unwrap();
        let surface_config = surface.get_default_config(&adapter, size.width, size.height).unwrap();
        surface.configure(&device, &surface_config);

        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
        let mut text = Text::new_without_auto_wakeup();
        let _text_edit_handle = text.add_text_edit("Type here...".to_string(), (50.0, 50.0), (400.0, 40.0), 0.0);

        Self { device, queue, surface, surface_config, window, text, text_renderer }
    }

    fn render(&mut self) {
        self.text.prepare_all(&mut self.text_renderer);
        self.text_renderer.gpu_load(&self.device, &self.queue);

        let frame = self.surface.get_current_texture().unwrap();
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor::default());

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &frame.texture.create_view(&TextureViewDescriptor::default()),
                    resolve_target: None,
                    ops: Operations { load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store },
                })],
                ..Default::default()
            });
            self.text_renderer.render(&mut pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}

struct Application { state: Option<State> }

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_none() {
            let window = Arc::new(event_loop.create_window(
                Window::default_attributes()
                    .with_title("Minimal Text Edit Example")
                    .with_inner_size(LogicalSize::new(500, 300))
            ).unwrap());
            self.state = Some(State::new(window));
        }
    }

    fn window_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, _: winit::window::WindowId, event: WindowEvent) {
        let state = self.state.as_mut().unwrap();

        state.text.handle_event(&event, &state.window);
        
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.surface_config.width = size.width;
                state.surface_config.height = size.height;
                state.surface.configure(&state.device, &state.surface_config);
                state.text_renderer.update_resolution(size.width as f32, size.height as f32);
            }
            WindowEvent::RedrawRequested => {
                state.render();
                state.window.request_redraw();
            },
            _ => {}
        }
    }
}
