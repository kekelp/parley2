use parley::TextStyle;
use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::{WindowEvent, ElementState},
    event_loop::EventLoop,
    window::Window,
    keyboard::ModifiersState,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { state: None })
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

    single_line_input: TextEditHandle,
    clipped_text_box: TextBoxHandle,

    big_text_style: StyleHandle,
    modifiers: ModifiersState,
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

        let white = [255,0,0,255];
        let mut text = Text::new_without_auto_wakeup();
        
        // Create a style
        let big_text_style_handle = text.add_style(TextStyle {
            font_size: 64.0,
            brush: ColorBrush(white),
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        }, None);

        // Create text boxes and get handles. Normally, the handle would be owned by a higher level struct representing a node in a GUI tree or something similar.
        let single_line_input = text.add_text_edit("".to_string(), (10.0, 15.0), (200.0, 35.0), 0.0);
        let editable_text_with_unicode = text.add_text_edit("Editable text 無限での座を含む全てのEditable text 無限での座を含む全てのEditable text 無限での座を含む全てのEditable text 無限での座を含む全てのEditable text 無限での座を含む全ての".to_string(), (300.0, 200.0), (400.0, 200.0), 0.0);
        let _info = text.add_text_box("Press Ctrl + D to disable the top edit box. Press Ctrl + H to toggle the fade effect on the clipped text box".to_string(), (10.0, 60.0), (200.0, 100.0), 0.0);
        let _help_text_edit = text.add_text_edit("Press Ctrl + Plus and Ctrl + Minus to adjust the size of the big text.".to_string(), (470.0, 60.0), (200.0, 150.0), 0.0);
        let shift_enter_text_edit = text.add_text_edit("Use Shift+Enter for newlines here".to_string(), (250.0, 60.0), (200.0, 100.0), 0.0);
        
        let clipped_text_box = text.add_text_box("Clipped text".to_string(), (10.0, 340.0), (300.0, 50.0), 0.0);
        
        // Using a &'static str here for this non-editable text box.
        let justified_static_text = text.add_text_box("Long static words, Long static words, Long static words, Long static words, ... (justified btw) ", (200.0, 400.0), (400.0, 150.0), 0.0);
        
        // Use the handles to access and edit text boxes. Accessing a box through a handle is a very fast operation, basically just an array access. There is no hashing involved.
        text.get_text_edit_mut(&single_line_input).set_single_line(true);
        text.get_text_edit_mut(&single_line_input).set_placeholder("Single line input".to_string());
        text.get_text_edit_mut(&editable_text_with_unicode).set_style(&big_text_style_handle);
        text.get_text_edit_mut(&shift_enter_text_edit).set_newline_mode(NewlineMode::ShiftEnter);
        
        text.get_text_box_mut(&clipped_text_box).set_style(&big_text_style_handle);
        text.get_text_box_mut(&clipped_text_box).text_mut();
        
        text.get_text_box_mut(&clipped_text_box).set_clip_rect(Some(parley::Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 30.0,
        }));

        text.get_text_style_mut(&big_text_style_handle).font_size = 32.0;

        text.get_text_box_mut(&justified_static_text).set_style(&big_text_style_handle);
        text.get_text_box_mut(&justified_static_text).set_alignment(Alignment::Justify);

        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text,

            single_line_input,
            clipped_text_box,
            big_text_style: big_text_style_handle,

            modifiers: ModifiersState::default(),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        self.text.handle_event(&event, &self.window);

        match event {
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.text_renderer.update_resolution(size.width as f32, size.height as f32);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                // Prepare the cpu-side text data for rendering.
                self.text.prepare_all(&mut self.text_renderer);
                // Load the text renderer's data on the gpu.
                self.text_renderer.gpu_load(&self.device, &self.queue);

                // A bunch of wgpu boilerplate to be able to draw on the screen.
                let surface_texture = self.surface.get_current_texture().unwrap();
                let view = surface_texture.texture.create_view(&TextureViewDescriptor::default());
                let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor { label: None });
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });

                    // Finally, render the text on the screen.
                    self.text_renderer.render(&mut pass);
                }

                // More boilerplate.
                self.queue.submit(Some(encoder.finish()));
                surface_texture.present();
                self.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && self.modifiers.control_key() {
                    if let Some(s) = event.text {
                        match s.as_str() {
                            "+" => {
                                self.text.get_text_style_mut(&self.big_text_style).font_size += 2.0;
                            }
                            "-" => {
                                let current_size = self.text.get_text_style(&self.big_text_style).font_size;
                                if current_size > 4.0 {
                                    self.text.get_text_style_mut(&self.big_text_style).font_size -= 2.0;
                                }
                            }
                            "d" => {
                                let is_disabled = self.text.get_text_edit(&self.single_line_input).disabled();
                                self.text.set_text_edit_disabled(&self.single_line_input, !is_disabled);
                            }
                            "h" => {
                                let current_fadeout = self.text.get_text_box(&self.clipped_text_box).fadeout_clipping();
                                self.text.get_text_box_mut(&self.clipped_text_box).set_fadeout_clipping(!current_fadeout);
                            }
                            _ => {}
                        }
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
            .with_title("hello world");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        window.set_ime_allowed(true);

        self.state = Some(State::new(window));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(state) = &mut self.state {
            state.window_event(event_loop, event);
        };
    }
}