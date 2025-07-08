use parley2::*;
use std::sync::Arc;
use parley::*;
use wgpu::*;
use winit::{dpi::LogicalSize, event::WindowEvent, event_loop::EventLoop, window::Window};

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
    text_layouts: Vec<Layout<ColorBrush>>,
    show_atlas: bool,
    current_layout: usize,
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
        let surface_config = surface.get_default_config(&adapter, physical_size.width, physical_size.height).unwrap();
        surface.configure(&device, &surface_config);

        let layout = vec![
            rich_layout(),
            layout(JAPANESE_TEXT),
            layout(CHINESE_TEXT),
            layout(CYRILLIC_TEXT),
            layout(TOO_ADVANCED),
        ];

        let text_renderer_params = TextRendererParams {
            atlas_page_size: AtlasPageSize::Flat(300), // tiny page to test out multi-page stuff
        };
        
        let text_renderer = TextRenderer::new_with_params(&device, &queue, surface_config.format, None, text_renderer_params);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text_layouts: layout,
            current_layout: 0,
            show_atlas: false,
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::F1) = event.logical_key {
                    if event.state.is_pressed() && ! event.repeat {
                        self.window.request_redraw();
                        self.show_atlas = ! self.show_atlas;
                    }
                }
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowLeft) = event.logical_key {
                    if event.state.is_pressed() && !event.repeat {
                        let len = self.text_layouts.len();
                        self.current_layout = (self.current_layout + len - 1) % len;
                        self.window.request_redraw();
                    }
                }
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowRight) = event.logical_key {
                    if event.state.is_pressed() && ! event.repeat {
                        self.current_layout = (self.current_layout + 1) % self.text_layouts.len();
                        self.window.request_redraw();
                    }
                }
            }
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.text_renderer.update_resolution(size.width as f32, size.height as f32);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame = self.surface.get_current_texture().unwrap();
                let view = frame.texture.create_view(&TextureViewDescriptor::default());

                let now = std::time::Instant::now();
                self.text_renderer.clear();
                self.text_renderer.prepare_layout(&self.text_layouts[self.current_layout], 50.0, 50.0);
                println!("prepare(): {:?}", now.elapsed());

                self.text_renderer.gpu_load(&self.device, &self.queue);

                let mut encoder = self
                    .device
                    .create_command_encoder(&CommandEncoderDescriptor { label: None });
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::GREEN),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });

                    if self.show_atlas {
                        self.text_renderer.gpu_load_atlas_debug(&self.device, &self.queue);
                        self.text_renderer.render_atlas_debug(&mut pass);
                    } else {
                        self.text_renderer.render(&mut pass);
                    }
                }

                self.queue.submit(Some(encoder.finish()));
                frame.present();
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

        // Set up window
        let (width, height) = (800, 600);
        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(width as f64, height as f64))
            .with_title("hello world");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

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

fn rich_layout() -> Layout<ColorBrush> {
    // const RICH_TEXT: &str = "Press F1 to see the atlas pages \n\
    const RICH_TEXT: &str = "Press the left and right arrows to cycle through different examples\n
    
    This example uses tiny atlas pages to test out the multi-page functions, but normally only a single page will ever be used.
    When a glyph would have to spill out into a new page, the atlas will evict glyphs from older frames, trying to make enough space to avoid spilling.
    This means that multiple pages only kick in when the whole atlas isn't enough to contain the glyphs needed in a single frame
    Conversely, if nothing is at risk of spilling, cache eviction doesn't activate at all.
    
    Lorem ipsum\tdolor sit amet, conse🤡💯🧠🔥ctetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in r🔥👁️👄👁️🥶🤣😂eprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidata🤣😂💅🙃🤦‍♀️✨t non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. O algo.";    
    let display_scale = 1.0;

    let max_advance = Some(500.0 * display_scale);

    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();

    let text_brush = ColorBrush([0, 0, 0, 255]);
    let mut builder = layout_cx.ranged_builder(&mut font_cx, &RICH_TEXT, display_scale, true);

    builder.push_default(StyleProperty::Brush(text_brush));
    builder.push_default(FontStack::from("system-ui"));
    builder.push_default(StyleProperty::LineHeight(LineHeight::MetricsRelative(1.2)));
    builder.push_default(StyleProperty::FontSize(24.0));
    builder.push(StyleProperty::FontWeight(parley::FontWeight::new(800.0)), 0..31);
    builder.push(StyleProperty::FontWeight(parley::FontWeight::new(600.0)), 32..105);

    let mut layout: Layout<ColorBrush> = builder.build(&RICH_TEXT);

    layout.break_all_lines(max_advance);
    layout.align(max_advance, Alignment::Start, AlignmentOptions::default());

    return layout;
}

const CYRILLIC_TEXT: &str = "Мунди деленит молестиае усу ад, пертинах глориатур диссентиас ет нец. Ессент иудицабит маиестатис яуи ад, про ут дицо лорем легере. Вис те цоммодо сцрипта цорпора, тритани интеллегат аргументум цу еум, меи те яуем феугаит. При дисцере интеллегат ат, аеяуе афферт фуиссет ех вих. Цу хас интегре тхеопхрастус. Диам волуптатибус про еа.";

const CHINESE_TEXT: &str = "此后，人民文学出版社和齐鲁书社的做法被诸多出版社效仿，可见文化部出版局1985年的一纸批文并没有打消各地出版社出版此书的念头。所以，1988年新闻出版署发出了《关于整理出版〈金瓶梅〉及其研究资料的通知》。《通知》首先说明《金瓶梅》及其研究资料的需求“日益增大”，“先后有十余家出版社向我署提出报告，分别要求出版《金瓶梅》的各种版本及改编本，包括图录、连环画及影视文学剧本等”，但话锋一转，明确提出“《金瓶梅》一书虽在文学史上占有重要地位，但该书存在大量自然主义的秽亵描写，不宜广泛印行";

const JAPANESE_TEXT: &str = "ヘッケはこれらのL-函数が全複素平面へ有理型接続を持ち、指標が自明であるときには s = 1 でオーダー 1 である極を持ち、それ以外では解析的であることを証明した。原始ヘッケ指標（原始ディリクレ指標に同じ方法である modulus に相対的に定義された）に対し、ヘッケは、これらのL-函数が指標の L-函数の函数等式を満たし、L-函数の複素共役指標であることを示した。 主イデアル上の座と、無限での座を含む全ての例外有限集合の上で 1 である単円の上への写像を取ることで、イデール類群の指標 ψ を考える。すると、ψ はイデアル群 IS の指標 χ を生成し、イデアル群は S 上に入らない素イデアル上の自由アーベル群となる。";

const TOO_ADVANCED: &str = "Whoops! Parley is not able to show this stuff properly yet. (on Linux, at least.)
 𒀀𐎠𐤀𓀀𐊀𐌀𐍁𐐀𐑀𐒀𐓀🪨🪩༺࿐ཀླུ𓂀𓃰꧁꧂𝕳𝖊𝖑𝖑𝖔𝓗𝓮𝓵𝓵𝓸𝔸𝕓𝕔ᚠᚢᚦᚨᚱᚲ᚛᚜ᚫᚹᛁᛋ𐤈𐤉𐤊𐤋𐌸𐌰𐌽𐌺𐍃𝌆𝍖ꜰʟᴀꜱᴋʇxǝʇ⣿⣷⣄⠋⠕⠝⠞⍼⎋ﬡﷺ𐊗𐊕𐊐𐊎𐊆𐊍𐎅𐎟𐎚𐎗𐎛𐬀𐬁𐬂𐬃𐡀𐡁𐡂𐡃𒈙𒐫𒊒𒄆𓏤𓆉𓀀𓀁𓀂𓀃ඞ⋮⋰⋱≋≌≍≎≏꧅꧞🜁🜂🜃🜄🝰🝱🝲🝳𖡄𖤍𗼇𗼈𗼉𗼊༄༅༆༇࿈࿉࿊࿋⟦⟧⟨⟩⟪⟫⦃⦄⦅⦆⦇⦈᯼᯽᯾᯿᰻᰼᰽᰾⯑⮾⮿⯀⯁⿰⿱⿲⿳⿴⿵⿶⿷⿸⿹⿺⿻
｜｝～Ｈｅｌｌｏ　Ｗｏｒｌｄ！";

fn layout(text: &str) -> Layout<ColorBrush> {
    let display_scale = 1.0;
    let max_advance = Some(500.0 * display_scale);
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();

    let text_brush = ColorBrush([0, 0, 0, 255]);
    let mut builder = layout_cx.ranged_builder(&mut font_cx, &text, display_scale, true);
    builder.push_default(StyleProperty::Brush(text_brush));
    builder.push_default(FontStack::from("system-ui"));
    builder.push_default(StyleProperty::LineHeight(LineHeight::MetricsRelative(1.2)));
    builder.push_default(StyleProperty::FontSize(24.0));

    let mut layout: Layout<ColorBrush> = builder.build(&text);

    layout.break_all_lines(max_advance);
    layout.align(max_advance, Alignment::Start, AlignmentOptions::default());

    return layout;
}
