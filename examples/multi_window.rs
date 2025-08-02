// This example shows how you can draw text on multiple windows by just making separate Text and TextRenderer structs. There is no further multi-window support at the moment. 

use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::EventLoop,
    window::Window,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { state: None })
        .unwrap();
}

struct WindowState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,
    
    text: Text,
    text_renderer: TextRenderer,
}

struct State {
    windows: Vec<WindowState>,
}

impl State {
    fn new() -> Self {
        Self {
            windows: Vec::new(),
        }
    }

    fn add_window(&mut self, window: Arc<Window>, window_title: &str) {
        let physical_size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let surface = instance
            .create_surface(window.clone())
            .expect("Create surface");
        let surface_config = surface
            .get_default_config(&adapter, physical_size.width, physical_size.height)
            .unwrap();
        surface.configure(&device, &surface_config);

        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
        let mut text = Text::new_without_auto_wakeup();
        
        let _text_box = text.add_text_box(format!("Text in {}", window_title), (50.0, 50.0), (400.0, 100.0), 0.0);
        let _text_edit = text.add_text_edit( format!("Edit text in {}", window_title), (50.0, 200.0), (400.0, 40.0), 0.0);

        self.windows.push(WindowState {
            device,
            queue,
            surface,
            surface_config,
            window,
            text,
            text_renderer,
        });
    }

    fn render_window(&mut self, window_id: winit::window::WindowId) {
        if let Some(window_state) = self.windows.iter_mut().find(|w| w.window.id() == window_id) {
            window_state.text.prepare_all(&mut window_state.text_renderer);
            window_state.text_renderer.gpu_load(&window_state.device, &window_state.queue);

            let frame = window_state.surface.get_current_texture().unwrap();
            let view = frame.texture.create_view(&TextureViewDescriptor::default());
            let mut encoder = window_state.device.create_command_encoder(&CommandEncoderDescriptor::default());

            {
                let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color::GREEN),
                            store: StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });

                window_state.text_renderer.render(&mut render_pass);
            }

            window_state.queue.submit(std::iter::once(encoder.finish()));
            frame.present();
            window_state.window.request_redraw();
        }
    }
}

struct Application {
    state: Option<State>,
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_none() {
            let mut state = State::new();
            
            let window1 = Arc::new(event_loop.create_window(
                Window::default_attributes()
                    .with_title("Multi-window Example - Window 1")
                    .with_inner_size(LogicalSize::new(500, 400))
                ).unwrap());
                
                let window2 = Arc::new(event_loop.create_window(
                    Window::default_attributes()
                    .with_title("Multi-window Example - Window 2")
                    .with_inner_size(LogicalSize::new(500, 400))
            ).unwrap());
            
            state.add_window(window1, "Window 1");
            state.add_window(window2, "Window 2");
            
            self.state = Some(state);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(state) = &mut self.state {
            if let Some(window_state) = state.windows.iter_mut().find(|w| w.window.id() == window_id) {
                window_state.text.handle_event(&event, &window_state.window);
            }
            
            match event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                }
                WindowEvent::Resized(physical_size) => {
                    if let Some(window_state) = state.windows.iter_mut().find(|w| w.window.id() == window_id) {
                        window_state.surface_config.width = physical_size.width.max(1);
                        window_state.surface_config.height = physical_size.height.max(1);
                        window_state.surface.configure(&window_state.device, &window_state.surface_config);
                        window_state.text_renderer.update_resolution(physical_size.width as f32,physical_size.height as f32);
                    }
                }
                WindowEvent::RedrawRequested => {
                    state.render_window(window_id);
                }
                _ => {}
            }
        }
    }
}