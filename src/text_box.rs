use std::{
    cell::RefCell, ops::Range, sync::{Arc, RwLock}, time::{Duration, Instant}
};

use parley::{Affinity, Alignment, AlignmentOptions, Selection, TextStyle};
use winit::{
    event::{Modifiers, WindowEvent},
    keyboard::{Key, NamedKey},
    platform::modifier_supplement::KeyEventExtModifierSupplement,
};

use crate::*;

const X_TOLERANCE: f64 = 35.0;

pub(crate) struct TextContext {
    layout_cx: LayoutContext<ColorBrush>,
    font_cx: FontContext,
}
impl TextContext {
    pub fn new() -> Self {
        Self {
            layout_cx: LayoutContext::new(),
            font_cx: FontContext::new(),
        }
    }
}

thread_local! {
    static TEXT_CX: RefCell<TextContext> = RefCell::new(TextContext::new());
}

pub(crate) fn with_text_cx<R>(f: impl FnOnce(&mut LayoutContext<ColorBrush>, &mut FontContext) -> R) -> R {
    let res = TEXT_CX.with_borrow_mut(|text_cx| f(&mut text_cx.layout_cx, &mut text_cx.font_cx));
    res
}

pub struct TextBox<T: AsRef<str>> {
    pub(crate) text: T,
    pub(crate) style: Style,
    pub(crate) shared_style_version: u32,
    pub selectable: bool,
    pub(crate) layout: Layout<ColorBrush>,
    pub(crate) needs_relayout: bool,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) max_advance: f32,
    pub depth: f32,
    pub(crate) selection: SelectionState,

    pub(crate) compose: Option<Range<usize>>,
    pub(crate) show_cursor: bool,
    pub(crate) cursor_visible: bool,
    pub(crate) width: Option<f32>,
    pub(crate) alignment: Alignment,
    pub(crate) start_time: Option<Instant>,
    pub(crate) blink_period: Duration,
    pub(crate) modifiers: Modifiers,
    pub(crate) scale: f32,
    pub(crate) history: TextEditHistory,
}

lazy_static::lazy_static! {
    pub static ref DEFAULT_TEXT_STYLE: SharedStyle = SharedStyle::new(TextStyle::default());
}

pub enum Style {
    Shared(SharedStyle),
    Unique(TextStyle<'static, ColorBrush>),
}
impl Default for Style {
    fn default() -> Self {
        Self::Shared(DEFAULT_TEXT_STYLE.clone())
    }
}
impl Style {
    pub fn with_text_style<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&TextStyle<'static, ColorBrush>, Option<u32>) -> R,
    {
        match self {
            Style::Shared(shared) => {
                let inner = shared.0.read().unwrap();
                f(&inner.style, Some(inner.version))
            }
            Style::Unique(style) => f(style, None),
        }
    }
}

// todo: this probably won't be needed actually
// when using it in a declarative library, after you change a style, you just redeclare everything and pass the new style to everyone that needs it
// (and they need to detect changes)
pub struct SharedStyle(Arc<RwLock<InnerStyle>>);
struct InnerStyle {
    style: TextStyle<'static, ColorBrush>,
    version: u32,
}
impl SharedStyle {
    pub fn new(style: TextStyle<'static, ColorBrush>) -> Self {
        Self(Arc::new(RwLock::new(InnerStyle { style, version: 0 })))
    }

    pub fn with_borrow_mut<R>(
        &self,
        f: impl FnOnce(&mut TextStyle<'static, ColorBrush>) -> R,
    ) -> R {
        let mut inner = self.0.write().unwrap();
        inner.version += 1;
        f(&mut inner.style)
    }
}
impl Clone for SharedStyle {
    fn clone(&self) -> Self {
        SharedStyle(self.0.clone())
    }
}

pub struct SelectionState {
    pub(crate) selection: Selection,
    pub(crate) prev_anchor: Option<Selection>,
    pub(crate) pointer_down: bool,
    pub(crate) focused: bool,
    pub(crate) last_click_time: Option<Instant>,
    pub(crate) click_count: u32,
    pub(crate) cursor_pos: (f32, f32),
}
impl SelectionState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            focused: false,
            last_click_time: None,
            click_count: 0,
            cursor_pos: (0.0, 0.0),
            selection: Default::default(),
            prev_anchor: Default::default(),
        }
    }
}

impl<T: AsRef<str>> TextBox<T> {
    pub fn new(text: T, pos: (f64, f64), max_advance: f32, depth: f32) -> Self {
        Self {
            text,
            shared_style_version: 0,
            layout: Layout::new(),
            selectable: true,
            needs_relayout: true,
            left: pos.0,
            top: pos.1,
            max_advance,
            depth,
            selection: SelectionState::new(),
            style: Style::default(),
            compose: Default::default(),
            show_cursor: true,
            cursor_visible: Default::default(),
            width: Default::default(),
            alignment: Default::default(),
            start_time: Default::default(),
            blink_period: Default::default(),
            modifiers: Default::default(),
            scale: Default::default(),
            history: TextEditHistory::default(),
        }
    }

    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        self.refresh_layout();
        &self.layout
    }

    pub fn refresh_layout(&mut self) {
        self.style.with_text_style(|style, version| {
            let shared_style_changed = if let Some(version) = version {
                self.shared_style_version == version
            } else { false };

            if self.needs_relayout || shared_style_changed {
                // todo: deduplicate
                with_text_cx(|layout_cx, font_cx| {
                    let mut builder =
                        layout_cx
                            .tree_builder(font_cx, 1.0, style);
        
                    builder.push_text(&self.text.as_ref());
        
                    let (mut layout, _) = builder.build();
        
                    layout.break_all_lines(Some(self.max_advance));
                    layout.align(
                        Some(self.max_advance),
                        Alignment::Start,
                        AlignmentOptions::default(),
                    );
        
                    self.layout = layout;
                    self.needs_relayout = false;
                });
            }
        });
    }

    pub fn update_layout(&mut self) {
        self.style.with_text_style(|style, _version| {

            // todo: deduplicate
            with_text_cx(|layout_cx, font_cx| {
                let mut builder =
                    layout_cx
                        .tree_builder(font_cx, 1.0, style);
    
                builder.push_text(&self.text.as_ref());
    
                let (mut layout, _) = builder.build();
    
                layout.break_all_lines(Some(self.max_advance));
                layout.align(
                    Some(self.max_advance),
                    Alignment::Start,
                    AlignmentOptions::default(),
                );
    
                self.layout = layout;
                self.needs_relayout = false;
            });
            
        });
    }

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent, modifiers: &Modifiers) {
        if !self.selectable {
            self.selection.focused = false;
            return;
        }

        self.refresh_layout();

        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if state.is_pressed() {
                    if *button == winit::event::MouseButton::Left {
                        let offset = (
                            self.selection.cursor_pos.0 as f64 - self.left,
                            self.selection.cursor_pos.1 as f64 - self.top,
                        );

                        if !self.layout.hit_bounding_box(offset) {
                            self.selection.focused = false;
                            self.selection
                                .set_selection(self.selection.selection.collapse());
                        }
                    }
                } else {
                    self.selection.pointer_down = false;
                }
            }
            _ => {}
        }

        if !self.selection.focused {
            return;
        }

        self.selection.handle_event(
            event,
            modifiers,
            &self.layout,
            self.left as f32,
            self.top as f32,
        );

        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {}
                #[allow(unused)]
                let mods_state = modifiers.state();
                let shift = mods_state.shift_key();
                let action_mod = if cfg!(target_os = "macos") {
                    mods_state.super_key()
                } else {
                    mods_state.control_key()
                };

                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                if action_mod {
                    match event.key_without_modifiers() {
                        Key::Character(c) => {
                            use clipboard_rs::{Clipboard, ClipboardContext};
                            match c.as_str() {
                                "c" if !shift => {
                                    if let Some(text) = self
                                        .text
                                        .as_ref()
                                        .get(self.selection.selection.text_range())
                                    {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                    }
                                }
                                "a" => {
                                    self.selection.selection = Selection::from_byte_index(
                                        &self.layout,
                                        0_usize,
                                        Affinity::default(),
                                    )
                                    .move_lines(&self.layout, isize::MAX, true);
                                }
                                _ => (),
                            }
                        }
                        _ => (),
                    };
                }
            }
            _ => {}
        }
    }

    pub fn try_grab_focus(&mut self, event: &WindowEvent, _modifiers: &Modifiers) -> bool {
        if !self.selectable {
            self.selection.focused = false;
            return false;
        }

        self.refresh_layout();
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Left && state.is_pressed() {
                    let offset = (
                        self.selection.cursor_pos.0 as f64 - self.left,
                        self.selection.cursor_pos.1 as f64 - self.top,
                    );

                    if self.layout.hit_bounding_box(offset) {
                        self.selection.pointer_down = true;
                        self.selection.focused = true;
                        return true;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let prev_pos = self.selection.cursor_pos;

                let cursor_pos = (position.x as f32, position.y as f32);
                self.selection.cursor_pos = cursor_pos;

                // macOS seems to generate a spurious move after selecting word?
                if self.selection.pointer_down && prev_pos != self.selection.cursor_pos {
                    let cursor_pos = (
                        cursor_pos.0 - self.left as f32,
                        cursor_pos.1 - self.top as f32,
                    );
                    self.selection.extend_selection_to_point(
                        &self.layout,
                        cursor_pos.0,
                        cursor_pos.1,
                        true,
                    );
                }
            }
            _ => {}
        }
        return false;
    }

    pub fn focused(&self) -> bool {
        self.selection.focused
    }

    pub fn set_shared_style(&mut self, style: &SharedStyle) {
        self.style = Style::Shared(style.clone());
        self.needs_relayout = true;
    }

    pub fn set_unique_style(&mut self, style: TextStyle<'static, ColorBrush>) {
        self.style = Style::Unique(style);
        self.needs_relayout = true;
    }
}

pub(crate) trait Ext1 {
    fn hit_bounding_box(&self, cursor_pos: (f64, f64)) -> bool;
}
impl Ext1 for Layout<ColorBrush> {
    fn hit_bounding_box(&self, top_left_corner: (f64, f64)) -> bool {
        let hit = top_left_corner.0 > -X_TOLERANCE
            && top_left_corner.0 < self.max_content_width() as f64 + X_TOLERANCE
            && top_left_corner.1 > 0.0
            && top_left_corner.1 < self.height() as f64;

        return hit;
    }
}

const MULTICLICK_DELAY: f64 = 0.4;

impl SelectionState {
    pub fn handle_event(
        &mut self,
        event: &winit::event::WindowEvent,
        modifiers: &Modifiers,
        layout: &Layout<ColorBrush>,
        left: f32,
        top: f32,
    ) {
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = modifiers.state().shift_key();
                if *button == winit::event::MouseButton::Left {
                    self.pointer_down = state.is_pressed();

                    let cursor_pos = (
                        self.cursor_pos.0 as f32 - left,
                        self.cursor_pos.1 as f32 - top,
                    );

                    if self.pointer_down {
                        let now = Instant::now();
                        if let Some(last) = self.last_click_time.take() {
                            if now.duration_since(last).as_secs_f64() < MULTICLICK_DELAY {
                                self.click_count = (self.click_count + 1) % 4;
                            } else {
                                self.click_count = 1;
                            }
                        } else {
                            self.click_count = 1;
                        }
                        self.last_click_time = Some(now);
                        let click_count = self.click_count;
                        match click_count {
                            2 => self.select_word_at_point(layout, cursor_pos.0, cursor_pos.1),
                            3 => self.select_line_at_point(layout, cursor_pos.0, cursor_pos.1),
                            _ => {
                                if shift {
                                    self.extend_selection_with_anchor(
                                        layout,
                                        cursor_pos.0,
                                        cursor_pos.1,
                                    )
                                } else {
                                    self.move_to_point(layout, cursor_pos.0, cursor_pos.1)
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return;
                }
                #[allow(unused)]
                let mods_state = modifiers.state();
                let shift = mods_state.shift_key();
                let action_mod = if cfg!(target_os = "macos") {
                    mods_state.super_key()
                } else {
                    mods_state.control_key()
                };

                if shift {
                    match &event.logical_key {
                        Key::Named(NamedKey::ArrowLeft) => {
                            if action_mod {
                                self.select_word_left(layout);
                            } else {
                                self.select_left(layout);
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if action_mod {
                                self.select_word_right(layout);
                            } else {
                                self.select_right(layout);
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            self.select_up(layout);
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            self.select_down(layout);
                        }
                        Key::Named(NamedKey::Home) => {
                            if action_mod {
                                self.select_to_text_start(layout);
                            } else {
                                self.select_to_line_start(layout);
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            if action_mod {
                                self.select_to_text_end(layout);
                            } else {
                                self.select_to_line_end(layout);
                            }
                        }
                        _ => (),
                    }
                }
            }
            _ => {}
        }
    }

    /// Move the cursor to the cluster boundary nearest this point in the layout.
    pub fn move_to_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.set_selection(Selection::from_point(layout, x, y));
    }

    pub fn select_word_at_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.set_selection(Selection::word_from_point(layout, x, y));
    }

    /// Select the physical line at the point.
    pub fn select_line_at_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        let line = Selection::line_from_point(layout, x, y);
        self.set_selection(line);
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub fn extend_selection_to_point(
        &mut self,
        layout: &Layout<ColorBrush>,
        x: f32,
        y: f32,
        keep_granularity: bool,
    ) {
        // FIXME: This is usually the wrong way to handle selection extension for mouse moves, but not a regression.
        self.set_selection(
            self.selection
                .extend_to_point(layout, x, y, keep_granularity),
        );
    }

    /// Extend the selection starting from the previous anchor, moving the selection focus point to the cluster boundary closest to point.
    ///
    /// Used for shift-click behavior.
    pub fn extend_selection_with_anchor(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        if let Some(prev_selection) = self.prev_anchor {
            self.set_selection_with_old_anchor(prev_selection);
        } else {
            self.prev_anchor = Some(self.selection);
        }
        // FIXME: This is usually the wrong way to handle selection extension for mouse moves, but not a regression.
        self.set_selection_with_old_anchor(self.selection.extend_to_point(layout, x, y, false));
    }

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    pub(crate) fn set_selection(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
        self.prev_anchor = None;
    }

    /// Update the selection without resetting the previous anchor.
    fn set_selection_with_old_anchor(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
    }

    fn set_selection_inner(&mut self, new_sel: Selection) {
        self.selection = new_sel;
    }

    /// Move the selection focus point to the start of the buffer.
    pub fn select_to_text_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.move_lines(layout, isize::MIN, true);
    }

    /// Move the selection focus point to the start of the physical line.
    pub fn select_to_line_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.line_start(layout, true);
    }

    /// Move the selection focus point to the end of the buffer.
    pub fn select_to_text_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.move_lines(layout, isize::MAX, true);
    }

    /// Move the selection focus point to the end of the physical line.
    pub fn select_to_line_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.line_end(layout, true);
    }

    /// Move the selection focus point up to the nearest cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub fn select_up(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_line(layout, true);
    }

    /// Move the selection focus point down to the nearest cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub fn select_down(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_line(layout, true);
    }

    /// Move the selection focus point to the next cluster left in visual order.
    pub fn select_left(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_visual(layout, true);
    }

    /// Move the selection focus point to the next cluster right in visual order.
    pub fn select_right(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_visual(layout, true);
    }

    /// Move the selection focus point to the next word boundary left.
    pub fn select_word_left(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_visual_word(layout, true);
    }

    /// Move the selection focus point to the next word boundary right.
    pub fn select_word_right(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_visual_word(layout, true);
    }
}

impl<T: AsRef<str>> TextBox<T> {
    pub fn text_real(&self) -> &T {
        &self.text
    }
    pub fn text_mut(&mut self) -> &mut T {
        &mut self.text
    }

    pub fn selection(&self) -> &Selection {
        &self.selection.selection
    }

    pub fn pos(&self) -> (f64, f64) {
        (self.left, self.top)
    }
}
