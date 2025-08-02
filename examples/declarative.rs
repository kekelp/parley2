/// Declarative pattern example showing 5x5 grid with different visibility patterns.

use parley::TextStyle;
use textslabs::*;
use std::{collections::HashMap, sync::Arc, time::Duration};
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::{WindowEvent, ElementState},
    event_loop::EventLoop,
    window::Window,
    keyboard::{NamedKey, Key},
};

#[derive(Debug)]
enum Pattern {
    All,
    Checkerboard,
    Cross,
    Border,
    Diagonal,
    Corners,
    Center,
    Rows,
}

impl Pattern {
    fn next(&self) -> Pattern {
        match self {
            Pattern::All => Pattern::Checkerboard,
            Pattern::Checkerboard => Pattern::Cross,
            Pattern::Cross => Pattern::Border,
            Pattern::Border => Pattern::Diagonal,
            Pattern::Diagonal => Pattern::Corners,
            Pattern::Corners => Pattern::Center,
            Pattern::Center => Pattern::Rows,
            Pattern::Rows => Pattern::All,
        }
    }
    
    fn prev(&self) -> Pattern {
        match self {
            Pattern::All => Pattern::Rows,
            Pattern::Checkerboard => Pattern::All,
            Pattern::Cross => Pattern::Checkerboard,
            Pattern::Border => Pattern::Cross,
            Pattern::Diagonal => Pattern::Border,
            Pattern::Corners => Pattern::Diagonal,
            Pattern::Center => Pattern::Corners,
            Pattern::Rows => Pattern::Center,
        }
    }
    
    
    fn should_show(&self, row: usize, col: usize) -> bool {
        match self {
            Pattern::All => true,
            Pattern::Checkerboard => (row + col) % 2 == 0,
            Pattern::Cross => row == 2 || col == 2,
            Pattern::Border => row == 0 || row == 4 || col == 0 || col == 4,
            Pattern::Diagonal => row == col,
            Pattern::Corners => (row == 0 || row == 4) && (col == 0 || col == 4),
            Pattern::Center => row >= 1 && row <= 3 && col >= 1 && col <= 3,
            Pattern::Rows => row % 2 == 0,
        }
    }
}

struct DeclarativeGrid {
    text: Text,
    text_renderer: TextRenderer,
    
    grid_handles: HashMap<(usize, usize), TextBoxHandle>,
    desc_handle: TextBoxHandle,
    comment_handle: TextBoxHandle,
    
    current_pattern: Pattern,
}

impl DeclarativeGrid {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let mut text = Text::new_without_auto_wakeup();
        let text_renderer = TextRenderer::new(device, queue, format);
        
        // Create styles
        let grid_style = text.add_style(TextStyle {
            font_size: 32.0,
            brush: ColorBrush([255, 255, 255, 255]),
            ..Default::default()
        }, None);
        
        let desc_style = text.add_style(TextStyle {
            font_size: 24.0,
            brush: ColorBrush([200, 200, 255, 255]),
            ..Default::default()
        }, None);
        
        let comment_style = text.add_style(TextStyle {
            font_size: 18.0,
            brush: ColorBrush([180, 255, 180, 255]),
            ..Default::default()
        }, None);
        
        let comment_handle = text.add_text_box(
            "This is a silly mostly AI-generated example to show how text boxes can be hidden or removed in a declarative style rather than imperative (or \"retained-mode\").",
            (50.0, 20.0),
            (700.0, 80.0),
            0.0,
        );
        text.get_text_box_mut(&comment_handle).set_can_hide(true);
        text.get_text_box_mut(&comment_handle).set_style(&comment_style);
        
        let desc_handle = text.add_text_box(
            "Use ← → to cycle patterns",
            (50.0, 550.0),
            (600.0, 40.0),
            0.0,
        );
        text.get_text_box_mut(&desc_handle).set_can_hide(true);
        text.get_text_box_mut(&desc_handle).set_style(&desc_style);
        
        let mut grid_handles = HashMap::new();
        for row in 0..5 {
            for col in 0..5 {
                let number = row * 5 + col + 1;
                let x = 50.0 + col as f64 * 80.0;
                let y = 120.0 + row as f64 * 80.0;
                
                let handle = text.add_text_box(
                    number.to_string(),
                    (x, y),
                    (60.0, 60.0),
                    0.0,
                );
                
                let mut text_box = text.get_text_box_mut(&handle);
                text_box.set_style(&grid_style);
                // Keep in memory when hidden. Without this, in addition to being auto-hidden, old text boxes would also be marked as to-remove, and removed on the next `Text::remove_old_nodes` call..
                text_box.set_can_hide(true);
                
                grid_handles.insert((row, col), handle);
            }
        }
        
        Self {
            text,
            text_renderer,
            grid_handles,
            desc_handle,
            comment_handle,
            current_pattern: Pattern::All,
        }
    }
    
    fn declare_frame(&mut self) {
        self.text.advance_frame_and_hide_boxes();
        
        for row in 0..5 {
            for col in 0..5 {
                if self.current_pattern.should_show(row, col) {
                    let handle = &self.grid_handles[&(row, col)];
                    self.text.refresh_text_box(handle);
                }
            }
        }
        
        self.text.refresh_text_box(&self.comment_handle);
        self.text.refresh_text_box(&self.desc_handle);
    }
    
    fn render(&mut self, view: &TextureView, device: &Device, queue: &Queue) {
        self.text.prepare_all(&mut self.text_renderer);
        self.text_renderer.gpu_load(device, queue);
        
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color { r: 0.1, g: 0.1, b: 0.1, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            
            self.text_renderer.render(&mut pass);
        }
        
        queue.submit(Some(encoder.finish()));
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { state: None })
        .unwrap();
}

struct State {
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,
    
    grid: DeclarativeGrid,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
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
        
        let grid = DeclarativeGrid::new(&device, &queue, surface_config.format);
        
        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            grid,
        }
    }
    
    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                // Declare current frame's state
                self.grid.declare_frame();
                
                // Render
                let frame = self.surface.get_current_texture().unwrap();
                let view = frame.texture.create_view(&TextureViewDescriptor::default());
                self.grid.render(&view, &self.device, &self.queue);
                frame.present();
                
                std::thread::sleep(Duration::from_millis(16));
                self.window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    match event.logical_key {
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.grid.current_pattern = self.grid.current_pattern.prev();
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.grid.current_pattern = self.grid.current_pattern.next();
                        }
                        Key::Named(NamedKey::Escape) => {
                            event_loop.exit();
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

struct Application {
    state: Option<State>,
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        
        let (width, height) = (800, 600);
        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(width as f64, height as f64))
            .with_title("Declarative");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        
        self.state = Some(State::new(window.clone()));
        window.request_redraw();
    }
    
    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(state) = &mut self.state {
            state.window_event(event_loop, event);
        }
    }
}