use crate::*;
use slab::Slab;
use std::time::Instant;
use winit::{event::{Modifiers, MouseButton, WindowEvent}, window::Window};

const MULTICLICK_DELAY: f64 = 0.4;
const MULTICLICK_TOLERANCE_SQUARED: f64 = 26.0;

/// Centralized struct that holds collections of [`TextBox`]es, [`TextEdit`]s, [`TextStyle2`]s.
/// 
/// For rendering, a [`TextRenderer`] is also needed.
pub struct Text {
    pub(crate) text_boxes: Slab<TextBox>,
    pub(crate) text_edits: Slab<TextEdit>,

    pub(crate) styles: Slab<(TextStyle2, TextEditStyle, u64)>,
    pub(crate) style_version_id_counter: u64,

    pub(crate) input_state: TextInputState,

    pub(crate) focused: Option<AnyBox>,
    pub(crate) mouse_hit_stack: Vec<(AnyBox, f32)>,
    
    pub(crate) text_changed: bool,
    pub(crate) using_frame_based_visibility: bool,
    pub(crate) decorations_changed: bool,

    pub(crate) current_frame: u64,
}

/// Handle for a text edit box.
/// 
/// Obtained when creating a text edit box with [`Text::add_text_edit()`].
/// 
/// Use with [`Text::get_text_edit()`] to get a reference to the corresponding [`TextEdit`]. 
#[derive(Debug)]
pub struct TextEditHandle {
    pub(crate) i: u32,
}

/// Handle for a text box.
/// 
/// Obtained when creating a text box with [`Text::add_text_box()`].
/// 
/// Use with [`Text::get_text_box()`] to get a reference to the corresponding [`TextBox`]
#[derive(Debug)]
pub struct TextBoxHandle {
    pub(crate) i: u32,
}


#[cfg(feature = "panic_on_handle_drop")]
impl Drop for TextEditHandle {
    fn drop(&mut self) {
        panic!(
            "TextEditHandle was dropped without being consumed! \
            This means that the corresponding text edit wasn't removed. To avoid leaking it, you should call Text::remove_text_edit(handle). \
            If you're intentionally leaking this text edit, you can use \
            std::mem::forget(handle) to skip the handle's drop() call and avoid this panic. \
            You can also disable this check by disabling the \"panic_on_handle_drop\" feature in Cargo.toml."
        );
    }
}

#[cfg(feature = "panic_on_handle_drop")]
impl Drop for TextBoxHandle {
    fn drop(&mut self) {
        panic!(
            "TextBoxHandle was dropped without being consumed! \
            This means that the corresponding text box wasn't removed. To avoid leaking it, you should call Text::remove_text_box(handle). \
            If you're intentionally leaking this text box, you can use \
            std::mem::forget(handle) to skip the handle's drop() call and avoid this panic. \
            You can also disable this check by disabling the \"panic_on_handle_drop\" feature in Cargo.toml."
        );
    }
}


/// Handle for a text style. Use with Text methods to apply styles to text.
pub struct StyleHandle {
    pub(crate) i: u32,
}
impl StyleHandle {
    pub(crate) fn sneak_clone(&self) -> Self {
        Self { i: self.i }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LastClickInfo {
    pub(crate) time: Instant,
    pub(crate) pos: (f64, f64),
    pub(crate) focused: Option<AnyBox>,
}

#[derive(Debug, Clone)]
pub(crate) struct MouseState {
    pub pointer_down: bool,
    pub cursor_pos: (f64, f64),
    pub last_click_info: Option<LastClickInfo>,
    pub click_count: u32,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            cursor_pos: (0.0, 0.0),
            last_click_info: None,
            click_count: 0,
        }
    }
}

/// Enum that can represent any type of text box (text box or text edit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyBox {
    TextEdit(u32),
    TextBox(u32),
}

#[derive(Debug, Clone)]
pub(crate) struct TextInputState {
    pub(crate) mouse: MouseState,
    pub(crate) modifiers: Modifiers,
}

impl TextInputState {
    pub fn new() -> Self {
        Self {
            mouse: MouseState::new(),
            modifiers: Modifiers::default(),
        }
    }

    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x, position.y);
                self.mouse.cursor_pos = cursor_pos;
            },

            WindowEvent::MouseInput { state, .. } => {
                self.mouse.pointer_down = state.is_pressed();
            },
            _ => {}
        }
    }
}

pub(crate) const DEFAULT_STYLE_I: usize = 0;
/// Pre-defined handle for the default text style.
pub const DEFAULT_STYLE_HANDLE: StyleHandle = StyleHandle { i: DEFAULT_STYLE_I as u32 };

impl Text {
    pub fn new() -> Self {
        let mut styles = Slab::with_capacity(10);
        let i = styles.insert((original_default_style(), TextEditStyle::default(), 0));
        debug_assert!(i == DEFAULT_STYLE_I);

        Self {
            text_boxes: Slab::with_capacity(10),
            text_edits: Slab::with_capacity(10),
            styles,
            style_version_id_counter: 0,
            input_state: TextInputState::new(),
            focused: None,
            mouse_hit_stack: Vec::with_capacity(6),
            text_changed: true,
            decorations_changed: true,
            current_frame: 1,
            using_frame_based_visibility: false,
        }
    }

    pub(crate) fn new_style_id(&mut self) -> u64 {
        self.style_version_id_counter += 1;
        self.style_version_id_counter
    }

    /// Add a text box and return a handle.
    /// 
    /// The handle can be used with [`Text::get_text_box()`] to get a reference to the [`TextBox`] that was added.
    /// 
    /// The [`TextBox`] must be manually removed by calling [`Text::remove_text_box()`].
    /// 
    /// `text` can be a `String`, a `&'static str`, or a `Cow<'static, str>`.
    #[must_use]
    pub fn add_text_box(&mut self, text: impl Into<Cow<'static, str>>, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextBoxHandle {
        let mut text_box = TextBox::new(text, pos, size, depth);
        text_box.last_frame_touched = self.current_frame;
        let i = self.text_boxes.insert(text_box) as u32;
        self.text_changed = true;
        TextBoxHandle { i }
    }

    /// Add a text edit and return a handle.
    /// 
    /// The handle can be used with [`Text::get_text_edit()`] to get a reference to the [`TextEdit`] that was added.
    /// 
    /// The [`TextEdit`] must be manually removed by calling [`Text::remove_text_edit()`].
    #[must_use]
    pub fn add_text_edit(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextEditHandle {
        let mut text_edit = TextEdit::new(text, pos, size, depth);
        text_edit.text_box.last_frame_touched = self.current_frame;
        let i = self.text_edits.insert(text_edit) as u32;
        self.text_changed = true;
        TextEditHandle { i }
    }


    /// Get a mutable reference to a text box.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    /// 
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_box_mut(&mut self, handle: &TextBoxHandle) -> &mut TextBox {
        self.text_changed = true;
        &mut self.text_boxes[handle.i as usize]
    }

    /// Get a reference to a text box.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_box(&self, handle: &TextBoxHandle) -> &TextBox {
        &self.text_boxes[handle.i as usize]
    }


    /// Get a mutable reference to a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_edit_mut(&mut self, handle: &TextEditHandle) -> &mut TextEdit {
        self.text_changed = true;
        &mut self.text_edits[handle.i as usize]
    }

    /// Get a reference to a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_edit(&self, handle: &TextEditHandle) -> &TextEdit {
        &self.text_edits[handle.i as usize]
    }

    /// Get the [`parley::Layout`] for a text box, recomputing it only if needed.
    pub fn get_text_box_layout(&mut self, handle: &TextBoxHandle) -> &Layout<ColorBrush> {
        let text_box = &mut self.text_boxes[handle.i as usize];
        refresh_text_box_layout(text_box, &self.styles);
        return &self.text_boxes[handle.i as usize].layout
    }


    /// Get the [`parley::Layout`] for a text edit box, recomputing it only if needed.
    pub fn get_text_edit_layout(&mut self, handle: &TextEditHandle) -> &Layout<ColorBrush> {
        let text_edit = &mut self.text_edits[handle.i as usize];
        refresh_text_edit_layout(text_edit, &self.styles);
        return &self.text_edits[handle.i as usize].text_box.layout
    }

    #[must_use]
    pub fn add_style(&mut self, text_style: TextStyle2, text_edit_style: Option<TextEditStyle>) -> StyleHandle {
        let text_edit_style = text_edit_style.unwrap_or_default();
        let new_id = self.new_style_id();
        let i = self.styles.insert((text_style, text_edit_style, new_id)) as u32;
        StyleHandle { i }
    }

    pub fn get_text_style(&self, handle: &StyleHandle) -> &TextStyle2 {
        &self.styles[handle.i as usize].0
    }

    pub fn get_text_style_mut(&mut self, handle: &StyleHandle) -> &mut TextStyle2 {
        self.styles[handle.i as usize].2 = self.new_style_id();
        self.text_changed = true;
        &mut self.styles[handle.i as usize].0
    }

    pub fn get_text_edit_style(&self, handle: &StyleHandle) -> &TextEditStyle {
        &self.styles[handle.i as usize].1
    }

    pub fn get_text_edit_style_mut(&mut self, handle: &StyleHandle) -> &mut TextEditStyle {
        self.styles[handle.i as usize].2 = self.new_style_id();
        self.text_changed = true;
        &mut self.styles[handle.i as usize].1
    }

    pub fn get_default_text_style(&self) -> &TextStyle2 {
        self.get_text_style(&DEFAULT_STYLE_HANDLE)
    }

    pub fn get_default_text_style_mut(&mut self) -> &mut TextStyle2 {
        self.get_text_style_mut(&DEFAULT_STYLE_HANDLE)
    }

    pub fn get_default_text_edit_style(&self) -> &TextEditStyle {
        self.get_text_edit_style(&DEFAULT_STYLE_HANDLE)
    }

    pub fn get_default_text_edit_style_mut(&mut self) -> &mut TextEditStyle {
        self.get_text_edit_style_mut(&DEFAULT_STYLE_HANDLE)
    }

    pub fn original_default_style(&self) -> TextStyle2 {
        original_default_style()
    }

    /// Advance an internal global frame counter that causes all text boxes to be implicitly marked as outdated and hidden.
    /// 
    /// You can then use [`Text::refresh_text_box()`] to "refresh" only the text boxes that should stay visible.
    /// 
    /// This allows to control the visibility of text boxes in a more "declarative" way.
    /// 
    /// Additionally, you can also use [`TextBox::set_can_hide()`] to decide if boxes should stay hidden in the background, or if they should marked as "to delete". You can the call [`Text::remove_old_nodes()`] to remove all the outdated text boxes that were marked as "to delete". 
    pub fn advance_frame_and_hide_boxes(&mut self) {
        self.current_frame += 1;
        self.using_frame_based_visibility = true;
    }

    /// Refresh a text box, causing it to stay visible even if [`Text::advance_frame_and_hide_boxes()`] was called.
    /// 
    /// Part of the "declarative" interface.  
    pub fn refresh_text_box(&mut self, handle: &TextBoxHandle) {
        if let Some(text_box) = self.text_boxes.get_mut(handle.i as usize) {
            text_box.last_frame_touched = self.current_frame;
        }
    }


    /// Refresh a text edit box, causing it to stay visible even if [`Text::advance_frame_and_hide_boxes()`] was called.
    /// 
    /// Part of the "declarative" interface.
    pub fn refresh_text_edit(&mut self, handle: &TextEditHandle) {
        if let Some(text_edit) = self.text_edits.get_mut(handle.i as usize) {
            text_edit.text_box.last_frame_touched = self.current_frame;
        }
    }


    /// Remove all text boxes that were made outdated by [`Text::advance_frame_and_hide_boxes()`], were not refreshed with [`Text::refresh_text_box()`], and were not set to remain as hidden with [`TextBox::set_can_hide()`].
    /// 
    /// Because [`Text::remove_old_nodes()`] mass-removes text boxes without consuming their handles, the handles become "dangling" and should not be reused. Using them in functions like [`Text::get_text_box()`] or [`Text::remove_text_box()`] will cause panics or incorrect results.
    /// 
    /// Only use this function if the structs holding the handles are managed in a way where you can be confident that the handles won't be kept around and reused.
    /// 
    /// On the other hand, it's fine to use the declarative system for *hiding* text boxes, but sticking to imperative [`Text::remove_text_box()`] calls to remove them.
    /// 
    /// [`Text::remove_old_nodes()`] is the only function that breaks the "no dangling handles" promise. If you use imperative [`Text::remove_text_box()`] calls and avoid `remove_old_nodes()`, then there is no way for the handle system to break.
    /// 

    pub fn remove_old_nodes(&mut self) {
        // Clear focus if the focused text box will be removed
        if let Some(focused) = self.focused {
            let should_clear_focus = match focused {
                AnyBox::TextBox(i) => {
                    if let Some(text_box) = self.text_boxes.get(i as usize) {
                        text_box.last_frame_touched != self.current_frame && !text_box.can_hide
                    } else {
                        true // Text box doesn't exist
                    }
                }
                AnyBox::TextEdit(i) => {
                    if let Some(text_edit) = self.text_edits.get(i as usize) {
                        text_edit.text_box.last_frame_touched != self.current_frame && !text_edit.text_box.can_hide
                    } else {
                        true // Text edit doesn't exist
                    }
                }
            };
            
            if should_clear_focus {
                self.focused = None;
            }
        }

        // Remove text boxes that are outdated and allowed to be removed
        self.text_boxes.retain(|_, text_box| {
            text_box.last_frame_touched == self.current_frame || text_box.can_hide
        });


        self.text_edits.retain(|_, text_edit| {
            text_edit.text_box.last_frame_touched == self.current_frame || text_edit.text_box.can_hide
        });
    }

    /// Remove a text box.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    pub fn remove_text_box(&mut self, handle: TextBoxHandle) {
        self.text_changed = true;
        if let Some(AnyBox::TextBox(i)) = self.focused {
            if i == handle.i {
                self.focused = None;
            }
        }
        self.text_boxes.remove(handle.i as usize);
        std::mem::forget(handle);
    }


    /// Remove a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    pub fn remove_text_edit(&mut self, handle: TextEditHandle) {
        self.text_changed = true;
        if let Some(AnyBox::TextEdit(i)) = self.focused {
            if i == handle.i {
                self.focused = None;
            }
        }
        self.text_edits.remove(handle.i as usize);
        std::mem::forget(handle);
    }

    /// Remove a text style.
    /// 
    /// If any text boxes are set to this style, they will revert to the default style.
    pub fn remove_style(&mut self, handle: StyleHandle) {
        self.styles.remove(handle.i as usize);
    }

    pub fn prepare_all(&mut self, text_renderer: &mut TextRenderer) {

        if ! self.text_changed && self.using_frame_based_visibility {
            // see if any text boxes were just hidden
            for (_i, text_edit) in self.text_edits.iter_mut() {
                if text_edit.text_box.last_frame_touched == self.current_frame - 1 {
                    self.text_changed = true;
                }
            }
            for (_i, text_box) in self.text_boxes.iter_mut() {
                if text_box.last_frame_touched == self.current_frame - 1 {
                    self.text_changed = true;
                }

            }
        }

        if self.text_changed {
            text_renderer.clear();
        } else if self.decorations_changed {
            text_renderer.clear_decorations_only();
        }

        if self.text_changed {
            for (_i, text_edit) in self.text_edits.iter_mut() {
                if !text_edit.hidden() && text_edit.text_box.last_frame_touched == self.current_frame {
                    refresh_text_edit_layout(text_edit, &self.styles);
                    text_renderer.prepare_text_edit_layout(text_edit);
                }
            }
            for (_i, text_box) in self.text_boxes.iter_mut() {
                if !text_box.hidden() && text_box.last_frame_touched == self.current_frame {
                    refresh_text_box_layout(text_box, &self.styles);
                    text_renderer.prepare_text_box_layout(text_box);
                }            
            }
        }

        if self.decorations_changed || self.text_changed {
            if let Some(focused) = self.focused {
                match focused {
                    AnyBox::TextEdit(i) => {
                        if ! &self.text_edits[i as usize].disabled() {
                            text_renderer.prepare_text_box_decorations(&self.text_edits[i as usize].text_box, true);
                        }
                    },
                    AnyBox::TextBox(i) => {
                        text_renderer.prepare_text_box_decorations(&self.text_boxes[i as usize], false);
                    },
                }
            }
        }

        self.text_changed = false;
        self.decorations_changed = false;

        self.using_frame_based_visibility = false;
    }

    /// Handle window events for text widgets.
    /// 
    /// This is the simple interface that works when text widgets aren't occluded by other objects.
    /// For complex z-ordering, use [`Text::find_topmost_text_box()`] and [`Text::handle_event_with_topmost()`], as described in the crate-level docs and shown in the `occlusion.rs` example.
    /// 
    /// Any events other than `winit::WindowEvent::MouseInput` can use either this method or the occlusion method interchangeably.
    pub fn handle_event(&mut self, event: &WindowEvent, window: &Window) {
        self.input_state.handle_event(event);

        if let WindowEvent::Resized(_) = event {
            self.text_changed = true;
        }

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                let new_focus = self.find_topmost_at_pos(self.input_state.mouse.cursor_pos);
                self.refocus(new_focus);
                self.handle_click_counting();
            }
        }

        if let Some(focused) = self.focused {
            self.refresh_anybox_layout(focused);
            self.handle_focused_event(focused, event, window);
            self.refresh_anybox_layout(focused);
            
            // todo: if we fix the layout thing, this can be done inside.
            if let AnyBox::TextEdit(i) = focused {
                if self.text_edits[i as usize].update_scroll_after_layout() {
                    self.text_changed = true;
                }
            }
        }
    }

    /// Find the topmost text box that would receive mouse events, if it wasn't occluded by any non-text-box objects.
    /// 
    /// Returns the handle of the topmost text widget at the event position, or None if no widget is hit.
    /// Use this with [`Text::handle_event_with_topmost()`] for complex z-ordering scenarios.
    pub fn find_topmost_text_box(&mut self, event: &WindowEvent) -> Option<AnyBox> {
        // Only handle mouse events that have a position
        let cursor_pos = match event {
            WindowEvent::MouseInput { .. } => self.input_state.mouse.cursor_pos,
            WindowEvent::CursorMoved { position, .. } => (position.x, position.y),
            _ => return None,
        };

        self.find_topmost_at_pos(cursor_pos)
    }

    /// Get the depth of a text box by its handle.
    /// 
    /// Used for comparing depths when integrating with other objects that might occlude text boxs.
    pub fn get_text_box_depth(&self, text_box_id: &AnyBox) -> f32 {
        match text_box_id {
            AnyBox::TextEdit(i) => self.text_edits.get(*i as usize).map(|te| te.depth()).unwrap_or(f32::MAX),
            AnyBox::TextBox(i) => self.text_boxes.get(*i as usize).map(|tb| tb.depth()).unwrap_or(f32::MAX),
        }
    }

    /// Handle window events with a pre-determined topmost text box.
    /// 
    /// Use this for complex z-ordering scenarios where text boxs might be occluded by other objects.
    /// Pass `Some(text_box_id)` if a text box should receive the event, or `None` if it's occluded.
    /// 
    /// If the text box is occluded, this function should still be called with `None`, so that text boxes can defocus.
    pub fn handle_event_with_topmost(&mut self, event: &WindowEvent, window: &Window, topmost_text_box: Option<AnyBox>) {
        self.input_state.handle_event(event);

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                self.refocus(topmost_text_box);
                self.handle_click_counting();
            }
        }

        if let Some(focused) = self.focused {    
            self.refresh_anybox_layout(focused);
            self.handle_focused_event(focused, event, window);
            self.refresh_anybox_layout(focused);
            
            if let AnyBox::TextEdit(i) = focused {
                if self.text_edits[i as usize].update_scroll_after_layout() {
                    self.text_changed = true;
                }
            }
        }
    }

    fn find_topmost_at_pos(&mut self, cursor_pos: (f64, f64)) -> Option<AnyBox> {
        self.mouse_hit_stack.clear();

        // Find all text widgets at this position
        for (i, text_edit) in self.text_edits.iter_mut() {
            if !text_edit.text_box.hidden && text_edit.text_box.last_frame_touched == self.current_frame && text_edit.text_box.hit_full_rect(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextEdit(i as u32), text_edit.depth()));
            }
        }
        for (i, text_box) in self.text_boxes.iter_mut() {
            if !text_box.hidden && text_box.last_frame_touched == self.current_frame && text_box.hit_bounding_box(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextBox(i as u32), text_box.depth()));
            }
        }

        // Find the topmost (lowest depth value)
        let mut topmost = None;
        let mut top_z = f32::MAX;
        for (id, z) in self.mouse_hit_stack.iter() {
            if *z < top_z {
                top_z = *z;
                topmost = Some(*id);
            }
        }

        topmost
    }

    fn refocus(&mut self, new_focus: Option<AnyBox>) {
        if new_focus != self.focused {
            if let Some(old_focus) = self.focused {
                self.remove_focus(old_focus);
            }
        }
        self.focused = new_focus;
        // todo: could skip some rerenders here if the old focus wasn't editable and had collapsed selection.
        self.decorations_changed = true;
    }

    fn handle_click_counting(&mut self) {
        let now = Instant::now();
        let current_pos = self.input_state.mouse.cursor_pos;
        
        if let Some(last_info) = self.input_state.mouse.last_click_info.take() {
            if now.duration_since(last_info.time).as_secs_f64() < MULTICLICK_DELAY 
                && last_info.focused == self.focused {
                let dx = current_pos.0 - last_info.pos.0;
                let dy = current_pos.1 - last_info.pos.1;
                let distance_squared = dx * dx + dy * dy;
                if distance_squared <= MULTICLICK_TOLERANCE_SQUARED {
                    self.input_state.mouse.click_count = (self.input_state.mouse.click_count + 1) % 4;
                } else {
                    self.input_state.mouse.click_count = 1;
                }
            } else {
                self.input_state.mouse.click_count = 1;
            }
        } else {
            self.input_state.mouse.click_count = 1;
        }
        
        self.input_state.mouse.last_click_info = Some(LastClickInfo {
            time: now,
            pos: current_pos,
            focused: self.focused,
        });
    }
    
    fn remove_focus(&mut self, old_focus: AnyBox) {
        match old_focus {
            AnyBox::TextEdit(i) => {
                self.text_edits[i as usize].text_box.reset_selection();
                self.text_edits[i as usize].show_cursor = false;
            },
            AnyBox::TextBox(i) => {
                self.text_boxes[i as usize].reset_selection();
            },
        }
    }
    
    fn handle_focused_event(&mut self, focused: AnyBox, event: &WindowEvent, window: &Window) {
        match focused {
            AnyBox::TextEdit(i) => {
                let result = self.text_edits[i as usize].handle_event(event, window, &self.input_state);
                if result.text_changed {
                    self.text_changed = true;
                    // todo: move this inside
                    self.text_edits[i as usize].text_box.needs_relayout = true;
                }
                if result.decorations_changed {
                    self.decorations_changed = true;
                }
            },
            AnyBox::TextBox(i) => {
                let result = self.text_boxes[i as usize].handle_event(event, window, &self.input_state);
                if result.text_changed {
                    self.text_changed = true;
                }
                if result.decorations_changed {
                    self.decorations_changed = true;
                }
            },
        }
    }

    /// Set the disabled state of a text edit box.
    /// 
    /// When disabled, the text edit will not respond to events and will be rendered with greyed out text.
    pub fn set_text_edit_disabled(&mut self, handle: &TextEditHandle, disabled: bool) {
        self.get_text_edit_mut(handle).set_disabled(disabled);
        if disabled {
            if let Some(AnyBox::TextEdit(e)) = self.focused {
                if e == handle.i {
                    self.get_text_edit_mut(handle).text_box.reset_selection();
                    self.focused = None;
                }
            }
        }

    }

    /// Returns whether any text was changed in the last frame.
    pub fn get_text_changed(&self) -> bool {
        self.text_changed
    }

    /// Programmatically set the text content of a text edit.
    /// This will replace all text and move the cursor to the end.
    pub fn set_text_edit_text(&mut self, handle: &TextEditHandle, new_text: String) {
        self.get_text_edit_mut(handle).set_text(new_text);
        self.text_changed = true;
    }

    pub(crate) fn refresh_anybox_layout(&mut self, handle: AnyBox) {
        match handle {
            AnyBox::TextEdit(i) => {
                let text_edit = &mut self.text_edits[i as usize];
                refresh_text_edit_layout(text_edit, &mut self.styles);
            },
            AnyBox::TextBox(i) => {
                let text_box = &mut self.text_boxes[i as usize];
                refresh_text_box_layout(text_box, &mut self.styles);
            },
        }
    }
}

pub fn refresh_text_edit_layout(text_edit: &mut TextEdit, styles: &Slab<(TextStyle2, TextEditStyle, u64)>) {
    let (style, edit_style, style_changed) = get_styles_for_element(&mut text_edit.text_box, &styles);

    let color_override = if text_edit.disabled {
        Some(edit_style.disabled_text_color)
    } else if text_edit.showing_placeholder {
        Some(edit_style.placeholder_text_color)
    } else {
        None
    };

    if text_edit.text_box.needs_relayout || style_changed {
        text_edit.text_box.rebuild_layout(style, color_override, text_edit.single_line);
    }
}

pub fn refresh_text_box_layout(text_box: &mut TextBox, styles: &Slab<(TextStyle2, TextEditStyle, u64)>) {
    let (style, _edit_style, style_changed) = get_styles_for_element(text_box, styles);
    if text_box.needs_relayout || style_changed {
        text_box.rebuild_layout(style, None, false);
    }
}


fn get_styles_for_element<'a>(text_box: &mut TextBox, styles: &'a Slab<(TextStyle2, TextEditStyle, u64)>) -> (&'a TextStyle2, &'a TextEditStyle, bool) {
    let style_handle = text_box.style.sneak_clone();
    let last_style_id = text_box.style_id;
    // todo: ABA problem here.
    let (text_style, text_edit_style, id) = styles.get(style_handle.i as usize).unwrap_or(&styles[DEFAULT_STYLE_HANDLE.i as usize]);
    let changed = last_style_id != *id;
    text_box.style_id = *id;
    (text_style, text_edit_style, changed)
}
