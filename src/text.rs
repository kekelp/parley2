use crate::*;
#[cfg(feature = "accessibility")]
use accesskit::{NodeId, TreeUpdate};
use slab::Slab;
#[cfg(feature = "accessibility")]
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use winit::{event::{Modifiers, MouseButton, WindowEvent}, window::Window};
use std::sync::{Arc, Weak};

const MULTICLICK_DELAY: f64 = 0.4;
const MULTICLICK_TOLERANCE_SQUARED: f64 = 26.0;

#[derive(Debug)]
pub(crate) struct StyleInner {
    pub(crate) text_style: TextStyle2,
    pub(crate) text_edit_style: TextEditStyle,
    pub(crate) version: u64,
}

/// Centralized struct that holds collections of [`TextBox`]es, [`TextEdit`]s, [`TextStyle2`]s.
/// 
/// For rendering, a [`TextRenderer`] is also needed.
pub struct Text {
    pub(crate) text_boxes: Slab<TextBoxInner>,
    pub(crate) text_edits: Slab<(TextEditInner, TextBoxInner)>,

    pub(crate) shared: Shared,

    pub(crate) style_version_id_counter: u64,

    pub(crate) input_state: TextInputState,

    pub(crate) focused: Option<AnyBox>,
    pub(crate) mouse_hit_stack: Vec<(AnyBox, f32)>,
    
    pub(crate) using_frame_based_visibility: bool,
    pub(crate) decorations_changed: bool,
    
    pub(crate) scrolled_moved_indices: Vec<AnyBox>,
    pub(crate) scroll_animations: Vec<ScrollAnimation>,

    pub(crate) current_visibility_frame: u64,
    pub(crate) cursor_blink_start: Option<Instant>,
    pub(crate) cursor_currently_blinked_out: bool,
    
    pub(crate) cursor_blink_timer: Option<CursorBlinkWaker>,

    pub(crate) slot_for_text_box_mut: Option<TextBoxMut<'static>>,

    #[cfg(feature = "accessibility")]
    pub(crate) accesskit_id_to_text_handle_map: HashMap<NodeId, AnyBox>,
}

/// Data that TextBoxMut and similar things need to have a reference to. Kept all together so that TextBoxMut and similar things can hold a single pointer to all of it.
/// 
/// A cooler way to do this would be to make the TextBoxMut be TextBoxMut { i: u32, text: &mut Text }. So you have access to the whole Text struct unconditionally, and you don't have to separate things this way. And to get the actual text box, you do self.text.text_boxes[i] every time. But we're trying this way this time
pub struct Shared {
    pub(crate) styles: Slab<StyleInner>,
    pub(crate) text_changed: bool,
    #[cfg(feature = "accessibility")]
    pub(crate) accesskit_tree_update: TreeUpdate,
    #[cfg(feature = "accessibility")]
    pub(crate) accesskit_focus_update: (Option<NodeId>, u64),
    pub(crate) current_event_number: u64,
    #[cfg(feature = "accessibility")]
    pub(crate) node_id_generator: fn() -> NodeId,
}

/// Handle for a text edit box.
/// 
/// Obtained when creating a text edit box with [`Text::add_text_edit()`].
/// 
/// Use with [`Text::get_text_edit()`] to get a reference to the corresponding [`TextEdit`]. 
#[derive(Debug, Clone)]
pub struct TextEditHandle {
    pub(crate) i: u32,
}

/// Handle for a text box.
/// 
/// Obtained when creating a text box with [`Text::add_text_box()`].
/// 
/// Use with [`Text::get_text_box()`] to get a reference to the corresponding [`TextBox`].
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
#[derive(Debug, Clone, Copy)]
pub struct StyleHandle {
    pub(crate) i: u32,
}
impl StyleHandle {
    #[allow(dead_code)]
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
/// 
///[`TextBoxHandle`] and [`TextEditHandle`] can be converted into `AnyBox`: `handle.into()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyBox {
    TextEdit(u32),
    TextBox(u32),
}

// todo: you can use this to clone a handle basically
pub trait IntoAnyBox {
    fn into_anybox(&self) -> AnyBox;
}
impl IntoAnyBox for TextBoxHandle {
    fn into_anybox(&self) -> AnyBox {
        AnyBox::TextBox(self.i)
    }
}
impl IntoAnyBox for TextEditHandle {
    fn into_anybox(&self) -> AnyBox {
        AnyBox::TextEdit(self.i)
    }
}
impl IntoAnyBox for AnyBox {
    fn into_anybox(&self) -> AnyBox {
        self.clone()
    }
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
    /// Create a new Text instance.
    /// 
    /// `window` is used to allow `Text` to wake up the `winit` event loop automatically when it needs to redraw a blinking cursor.
    /// 
    /// In applications that don't pause their event loops, like games, there is no need to use the wakeup system, so you can use [`Text::new_without_blink_wakeup`] instead.
    /// 
    /// You can also handle cursor wakeups manually in your winit event loop with winit's `ControlFlow::WaitUntil` and [`Text::time_until_next_cursor_blink`]. See the `event_loop_smart.rs` example.
    pub fn new(window: Arc<Window>) -> Self {
        Self::new_with_option(Some(window))
    }

    /// Create a new Text instance without cursor blink wakeup.
    /// 
    /// Use this function for applications that don't pause their event loops, like games, or when handling cursor wakeups manually with winit's `ControlFlow::WaitUntil` and [`Text::time_until_next_cursor_blink`]. See the `event_loop_smart.rs` example.
    pub fn new_without_auto_wakeup() -> Self {
        Self::new_with_option(None)
    }

    pub(crate) fn new_with_option(window: Option<Arc<Window>>) -> Self {
        let mut styles = Slab::with_capacity(10);
        let i = styles.insert(StyleInner {
            text_style: original_default_style(),
            text_edit_style: TextEditStyle::default(),
            version: 0,
        });
        debug_assert!(i == DEFAULT_STYLE_I);

        let cursor_blink_timer = window.map(|window| CursorBlinkWaker::new(Arc::downgrade(&window)));

        Self {
            text_boxes: Slab::with_capacity(10),
            text_edits: Slab::with_capacity(10),
            style_version_id_counter: 0,
            input_state: TextInputState::new(),
            focused: None,
            mouse_hit_stack: Vec::with_capacity(6),
            decorations_changed: true,
            scrolled_moved_indices: Vec::new(),
            scroll_animations: Vec::new(),
            current_visibility_frame: 1,
            using_frame_based_visibility: false,
            cursor_blink_start: None,
            cursor_currently_blinked_out: false,
            cursor_blink_timer,

            slot_for_text_box_mut: None,

            #[cfg(feature = "accessibility")]
            accesskit_id_to_text_handle_map: HashMap::with_capacity(50),

            shared: Shared {
                styles,
                text_changed: true,
                #[cfg(feature = "accessibility")]
                accesskit_focus_update: (Some(NodeId(0)), 0),
                current_event_number: 1,
                #[cfg(feature = "accessibility")]
                node_id_generator: crate::accessibility::next_node_id,
                #[cfg(feature = "accessibility")]
                accesskit_tree_update: TreeUpdate {
                    nodes: Vec::new(),
                    tree: None,
                    focus: NodeId(0),
                },
            },
        }
    }


    pub(crate) fn new_style_version(&mut self) -> u64 {
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
        let mut text_box = TextBoxInner::new(text, pos, size, depth);
        text_box.last_frame_touched = self.current_visibility_frame;
        text_box.style_version = self.shared.styles[text_box.style.i as usize].version;
        let i = self.text_boxes.insert(text_box) as u32;
        self.shared.text_changed = true;
        TextBoxHandle { i }
    }

    /// Add a text edit and return a handle.
    /// 
    /// The handle can be used with [`Text::get_text_edit()`] to get a reference to the [`TextEdit`] that was added.
    /// 
    /// The [`TextEdit`] must be manually removed by calling [`Text::remove_text_edit()`].
    #[must_use]
    pub fn add_text_edit(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextEditHandle {
        let (text_edit, mut text_box) = TextEditInner::new(text, pos, size, depth);
        text_box.last_frame_touched = self.current_visibility_frame;
        text_box.style_version = self.shared.styles[text_box.style.i as usize].version;
        let i = self.text_edits.insert((text_edit, text_box)) as u32;
        self.shared.text_changed = true;
        TextEditHandle { i }
    }




    /// Get a mutable reference to a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_edit_mut(&mut self, handle: &TextEditHandle) -> TextEditMut {
        self.shared.text_changed = true;
        self.get_full_text_edit(handle)
    }

    /// Get a reference to a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_edit(&mut self, handle: &TextEditHandle) -> TextEdit {
        let (text_edit_inner, text_box_inner) = self.text_edits.get_mut(handle.i as usize).unwrap();
        let text_box = TextBox { inner: text_box_inner, shared: &mut self.shared };
        TextEdit { inner: text_edit_inner, text_box }
    }

    #[must_use]
    pub fn add_style(&mut self, text_style: TextStyle2, text_edit_style: Option<TextEditStyle>) -> StyleHandle {
        let text_edit_style = text_edit_style.unwrap_or_default();
        let new_version = self.new_style_version();
        let i = self.shared.styles.insert(StyleInner {
            text_style,
            text_edit_style,
            version: new_version,
        }) as u32;
        StyleHandle { i }
    }

    pub fn get_text_style(&self, handle: &StyleHandle) -> &TextStyle2 {
        &self.shared.styles[handle.i as usize].text_style
    }

    pub fn get_text_style_mut(&mut self, handle: &StyleHandle) -> &mut TextStyle2 {
        self.shared.styles[handle.i as usize].version = self.new_style_version();
        self.shared.text_changed = true;
        &mut self.shared.styles[handle.i as usize].text_style
    }

    pub fn get_text_edit_style(&self, handle: &StyleHandle) -> &TextEditStyle {
        &self.shared.styles[handle.i as usize].text_edit_style
    }

    pub fn get_text_edit_style_mut(&mut self, handle: &StyleHandle) -> &mut TextEditStyle {
        self.shared.styles[handle.i as usize].version = self.new_style_version();
        self.shared.text_changed = true;
        &mut self.shared.styles[handle.i as usize].text_edit_style
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
        self.current_visibility_frame += 1;
        self.using_frame_based_visibility = true;
    }

    /// Refresh a text box, causing it to stay visible even if [`Text::advance_frame_and_hide_boxes()`] was called.
    /// 
    /// Part of the "declarative" interface.  
    pub fn refresh_text_box(&mut self, handle: &TextBoxHandle) {
        if let Some(text_box) = self.text_boxes.get_mut(handle.i as usize) {
            text_box.last_frame_touched = self.current_visibility_frame;
        }
    }


    /// Refresh a text edit box, causing it to stay visible even if [`Text::advance_frame_and_hide_boxes()`] was called.
    /// 
    /// Part of the "declarative" interface.
    pub fn refresh_text_edit(&mut self, handle: &TextEditHandle) {
        if let Some((_text_edit, text_box)) = self.text_edits.get_mut(handle.i as usize) {
            text_box.last_frame_touched = self.current_visibility_frame;
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
                        text_box.last_frame_touched != self.current_visibility_frame && !text_box.can_hide
                    } else {
                        true // Text box doesn't exist
                    }
                }
                AnyBox::TextEdit(i) => {
                    if let Some((_text_edit, text_box)) = self.text_edits.get(i as usize) {
                        text_box.last_frame_touched != self.current_visibility_frame && !text_box.can_hide
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
            text_box.last_frame_touched == self.current_visibility_frame || text_box.can_hide
        });


        self.text_edits.retain(|_, (_text_edit, text_box)| {
            text_box.last_frame_touched == self.current_visibility_frame || text_box.can_hide
        });
    }

    /// Remove a text box.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    pub fn remove_text_box(&mut self, handle: TextBoxHandle) {
        self.shared.text_changed = true;
        if let Some(AnyBox::TextBox(i)) = self.focused {
            if i == handle.i {
                self.focused = None;
            }
        }
        
        // Remove from accessibility mapping if it exists
        #[cfg(feature = "accessibility")]
        if let Some(text_box) = self.text_boxes.get(handle.i as usize) {
            if let Some(accesskit_id) = text_box.accesskit_id {
                self.accesskit_id_to_text_handle_map.remove(&accesskit_id);
            }
        }
        
        self.text_boxes.remove(handle.i as usize);
        std::mem::forget(handle);
    }


    /// Remove a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    pub fn remove_text_edit(&mut self, handle: TextEditHandle) {
        self.shared.text_changed = true;
        if let Some(AnyBox::TextEdit(i)) = self.focused {
            if i == handle.i {
                self.focused = None;
            }
        }
        
        // Remove from accessibility mapping if it exists
        #[cfg(feature = "accessibility")]
        if let Some((_text_edit, text_box)) = self.text_edits.get(handle.i as usize) {
            if let Some(accesskit_id) = text_box.accesskit_id {
                self.accesskit_id_to_text_handle_map.remove(&accesskit_id);
            }
        }
        
        self.text_edits.remove(handle.i as usize);
        std::mem::forget(handle);
    }

    /// Remove a text style.
    /// 
    /// If any text boxes are set to this style, they will revert to the default style.
    pub fn remove_style(&mut self, handle: StyleHandle) {
        self.shared.styles.remove(handle.i as usize);
    }

    pub fn prepare_all(&mut self, text_renderer: &mut TextRenderer) {
        if ! self.shared.text_changed && self.using_frame_based_visibility {
            // see if any text boxes were just hidden
            for (_i, (_text_edit, text_box)) in self.text_edits.iter_mut() {
                if text_box.last_frame_touched == self.current_visibility_frame - 1 {
                    self.shared.text_changed = true;
                }
            }
            for (_i, text_box) in self.text_boxes.iter_mut() {
                if text_box.last_frame_touched == self.current_visibility_frame - 1 {
                    self.shared.text_changed = true;
                }

            }
        }

        
        // decorations
        let (show_cursor, blink_changed) = self.cursor_blinked_out(true);

        if self.shared.text_changed {
            text_renderer.clear();
        } else if self.decorations_changed || !self.scrolled_moved_indices.is_empty() || blink_changed {
            text_renderer.clear_decorations_only();
        }

        if self.decorations_changed || self.shared.text_changed  || !self.scrolled_moved_indices.is_empty() || blink_changed {
            if let Some(focused) = self.focused {
                match focused {
                    AnyBox::TextEdit(i) => {
                        let handle = TextEditHandle { i: i as u32 };
                        let text_edit = self.get_full_text_edit(&handle);
                        text_renderer.prepare_text_box_decorations(&text_edit.text_box, show_cursor);
                    },
                    AnyBox::TextBox(i) => {
                        let handle = TextBoxHandle { i: i as u32 };
                        let text_box = self.get_full_text_box(&handle);
                        text_renderer.prepare_text_box_decorations(&text_box, false);
                    },
                }
            }
        }

        // if only scrolling or movement occurred, move quads in-place
        if !self.shared.text_changed {
            if !self.scrolled_moved_indices.is_empty() {
                self.handle_scroll_fast_path(text_renderer);
            }

        } else {
        // if self.shared.text_changed || !self.scrolled_moved_indices.is_empty(){

            let current_frame = self.current_visibility_frame;
            if self.shared.text_changed {
                for (_, text_edit) in self.text_edits.iter_mut() {
                    let mut text_edit = get_full_text_edit_free_function_but_for_iterating((&mut text_edit.0, &mut text_edit.1), &mut self.shared);
                    if !text_edit.hidden() && text_edit.text_box.inner.last_frame_touched == current_frame {
                        text_renderer.prepare_text_edit_layout(&mut text_edit);
                    }
                }

                for (_, text_box) in self.text_boxes.iter_mut() {
                    let mut text_box = get_full_text_box_free_function_but_for_iterating(text_box, &mut self.shared);
                    if !text_box.hidden() && text_box.inner.last_frame_touched == current_frame {
                        text_renderer.prepare_text_box_layout(&mut text_box);
                    }
                }
            }
        }

        self.shared.text_changed = false;
        self.decorations_changed = false;
        
        // Only clear scrolled indices when all animations are finished
        self.clear_finished_scroll_animations();

        self.using_frame_based_visibility = false;
    }

    /// Fast path for handling scroll-only changes by moving quads in-place
    fn handle_scroll_fast_path(&mut self, text_renderer: &mut TextRenderer) {
        for any_box in &self.scrolled_moved_indices {
            match any_box {
                AnyBox::TextEdit(i) => {
                    if let Some((_text_edit_inner, text_box_inner)) = self.text_edits.get_mut(*i as usize) {
                        move_quads_for_scroll(text_renderer, &mut text_box_inner.quad_storage, text_box_inner.scroll_offset);
                    }
                },
                AnyBox::TextBox(i) => {
                    if let Some(text_box_inner) = self.text_boxes.get_mut(*i as usize) {
                        move_quads_for_scroll(text_renderer, &mut text_box_inner.quad_storage, text_box_inner.scroll_offset);
                    }
                },
            }
        }
    }

    /// Clear scroll indices only for elements that have finished their animations
    fn clear_finished_scroll_animations(&mut self) {
        self.scrolled_moved_indices.retain(|any_box| {
            match any_box {
                AnyBox::TextEdit(i) => {
                    // Keep in list if any animation is still running for this text edit
                    self.scroll_animations.iter().any(|anim| anim.handle.i == *i)
                },
                AnyBox::TextBox(_i) => {
                    // Text boxes don't have animations, so they can be cleared immediately
                    false
                },
            }
        });
    }

    /// Handle window events for text widgets.
    /// 
    /// This is the simple interface that works when text widgets aren't occluded by other objects.
    /// For complex z-ordering, use [`Text::find_topmost_text_box()`] and [`Text::handle_event_with_topmost()`], as described in the crate-level docs and shown in the `occlusion.rs` example.
    /// 
    /// Any events other than `winit::WindowEvent::MouseInput` can use either this method or the occlusion method interchangeably.
    pub fn handle_event(&mut self, event: &WindowEvent, window: &Window) -> EventResult {
        let mut result = EventResult::nothing();

        self.shared.current_event_number += 1;
        
        self.input_state.handle_event(event);

        if let WindowEvent::Resized(_) = event {
            self.shared.text_changed = true;
            result.need_rerender = true;
        }

        // update smooth scrolling animations
        if let WindowEvent::RedrawRequested = event {
            let animation_updated = self.update_smooth_scrolling();
            if animation_updated {
                window.request_redraw();
                result.need_rerender = true;
            }
            // If animations are still running, we need to keep rerendering
            if self.get_max_animation_duration().is_some() {
                result.need_rerender = true;
            }
        }

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                let new_focus = self.find_topmost_at_pos(self.input_state.mouse.cursor_pos);
                if new_focus.is_some() {
                    result.consumed = true;
                    result.need_rerender = true;
                }
                self.refocus(new_focus);
                self.handle_click_counting();
            }
        }

        if let WindowEvent::MouseWheel { .. } = event {
            let hovered = self.find_topmost_at_pos(self.input_state.mouse.cursor_pos);
            if let Some(hovered_widget) = hovered {
                result.consumed = true;
                self.handle_hovered_event(hovered_widget, event, window, &mut result);
            }
            return result;
        }

        if let Some(focused) = self.focused {
            result.consumed = true;
            self.handle_focused_event(focused, event, window, &mut result);

            #[cfg(feature = "accessibility")] {   
                // todo: not the best, this includes decoration changes and stuff.
                if result.need_rerender {
                    self.push_ak_update_for_focused(focused);
                }
            }
        }

        let (_, changed) = self.cursor_blinked_out(false);
        if changed {
            // When using EventProxy timer, we don't need to set wake_up_in
            if self.cursor_blink_timer.is_none() {
                result.wake_up_event_loop_in = Some(Duration::from_millis(CURSOR_BLINK_TIME_MILLIS));
            }
            // result.need_rerender = true;
        }
        
        result
    }

    #[cfg(feature = "accessibility")]
    fn get_accesskit_id(&mut self, i: AnyBox) -> Option<NodeId> {
        return match i {
            AnyBox::TextEdit(i) => {
                let handle = TextEditHandle { i: i as u32 };
                let text_edit = get_full_text_edit_free_function(&mut self.text_edits, &mut self.shared, &handle);
                text_edit.accesskit_id()
            },
            AnyBox::TextBox(i) => {
                let handle = TextBoxHandle { i: i as u32 };
                let text_box = get_full_text_box_free_function(&mut self.text_boxes, &mut self.shared, &handle);
                text_box.accesskit_id()
            },
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
            AnyBox::TextEdit(i) => self.text_edits.get(*i as usize).map(|(_te, tb)| tb.depth).unwrap_or(f32::MAX),
            AnyBox::TextBox(i) => self.text_boxes.get(*i as usize).map(|tb| tb.depth).unwrap_or(f32::MAX),
        }
    }

    /// Handle window events with a pre-determined topmost text box.
    /// 
    /// Use this for complex z-ordering scenarios where text boxs might be occluded by other objects.
    /// Pass `Some(text_box_id)` if a text box should receive the event, or `None` if it's occluded.
    /// 
    /// If the text box is occluded, this function should still be called with `None`, so that text boxes can defocus.
    pub fn handle_event_with_topmost(&mut self, event: &WindowEvent, window: &Window, topmost_text_box: Option<AnyBox>) -> EventResult {
        let mut result = EventResult::nothing();
        
        self.input_state.handle_event(event);

        // update smooth scrolling animations
        if let WindowEvent::RedrawRequested = event {
            let animation_updated = self.update_smooth_scrolling();
            if animation_updated {
                window.request_redraw();
                result.need_rerender = true;
            }
            // If animations are still running, we need to keep rerendering
            if self.get_max_animation_duration().is_some() {
                result.need_rerender = true;
            }
        }

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                if topmost_text_box.is_some() {
                    result.consumed = true;
                    result.need_rerender = true;
                }
                self.refocus(topmost_text_box);
                self.handle_click_counting();
            }
        }

        if let WindowEvent::MouseWheel { .. } = event {
            if let Some(hovered_widget) = topmost_text_box {
                result.consumed = true;
                self.handle_hovered_event(hovered_widget, event, window, &mut result);
            }
        }

        if let Some(focused) = self.focused {
            result.consumed = true;
            self.handle_focused_event(focused, event, window, &mut result);
        }
        
        result
    }

    fn find_topmost_at_pos(&mut self, cursor_pos: (f64, f64)) -> Option<AnyBox> {
        self.mouse_hit_stack.clear();

        // Find all text widgets at this position
        for (i, (_text_edit, text_box)) in self.text_edits.iter_mut() {
            if !text_box.hidden && text_box.last_frame_touched == self.current_visibility_frame && text_box.hit_full_rect(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextEdit(i as u32), text_box.depth));
            }
        }
        for (i, text_box) in self.text_boxes.iter_mut() {
            if !text_box.hidden && text_box.last_frame_touched == self.current_visibility_frame && text_box.hit_bounding_box(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextBox(i as u32), text_box.depth));
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
        let focus_changed = new_focus != self.focused;
        
        if focus_changed {
            if let Some(old_focus) = self.focused {
                self.remove_focus(old_focus);
            }
        }

        self.focused = new_focus;
        
        if focus_changed {
            // todo: could skip some rerenders here if the old focus wasn't editable and had collapsed selection.
            self.decorations_changed = true;
            self.reset_cursor_blink();

            #[cfg(feature = "accessibility")]
            {
                let new_focus_ak_id = new_focus.and_then(|new_focus| self.get_accesskit_id(new_focus));
                self.shared.accesskit_focus_update = (new_focus_ak_id, self.shared.current_event_number);
            }
        }
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
                let handle = TextEditHandle { i: i as u32 };
                let mut text_edit = self.get_full_text_edit(&handle);
                text_edit.text_box.reset_selection();
                text_edit.inner.show_cursor = false;
            },
            AnyBox::TextBox(i) => {
                let handle = TextBoxHandle { i: i as u32 };
                let mut text_box = self.get_full_text_box(&handle);
                text_box.reset_selection();
            },
        }
    }
    
    fn handle_hovered_event(&mut self, hovered: AnyBox, event: &WindowEvent, window: &Window, result: &mut EventResult) {
        // scroll wheel event
        if let WindowEvent::MouseWheel { .. } = event {
            match hovered {
                AnyBox::TextEdit(i) => {
                    let handle = TextEditHandle { i: i as u32 };
                    let did_scroll = self.handle_text_edit_scroll_event(&handle, event, window);
                    if did_scroll {
                        result.need_rerender = true;
                        self.decorations_changed = true;
                        self.scrolled_moved_indices.push(AnyBox::TextEdit(i));
                    }
                },
                AnyBox::TextBox(_) => {}
            }
        }
    }

    fn handle_focused_event(&mut self, focused: AnyBox, event: &WindowEvent, window: &Window, result: &mut EventResult) {
        match focused {
            AnyBox::TextEdit(i) => {
                let handle = TextEditHandle { i: i as u32 };
                let mut text_edit = get_full_text_edit_free_function(&mut self.text_edits, &mut self.shared, &handle);

                let text_result = text_edit.handle_event(event, window, &self.input_state);
                if text_result.text_changed {
                    self.shared.text_changed = true;
                    result.need_rerender = true;
                    self.reset_cursor_blink();
                }
                if text_result.decorations_changed {
                    self.decorations_changed = true;
                    result.need_rerender = true;
                    self.reset_cursor_blink();
                }
                if !text_result.text_changed && text_result.scrolled {
                    self.scrolled_moved_indices.push(AnyBox::TextEdit(i));
                    result.need_rerender = true;
                }
            },
            AnyBox::TextBox(i) => {
                let handle = TextBoxHandle { i: i as u32 };
                let mut text_box = get_full_text_box_free_function(&mut self.text_boxes, &mut self.shared, &handle);

                let text_result = text_box.handle_event(event, window, &self.input_state);
                if text_result.text_changed {
                    self.shared.text_changed = true;
                    result.need_rerender = true;
                }
                if text_result.decorations_changed {
                    self.decorations_changed = true;
                    result.need_rerender = true;
                }
                if !text_result.text_changed && text_result.scrolled {
                    self.scrolled_moved_indices.push(AnyBox::TextBox(i));
                    result.need_rerender = true;
                }
            },
        }
    }

    /// Set the disabled state of a text edit box.
    /// 
    /// When disabled, the text edit will not respond to events and will be rendered with greyed out text.
    pub fn set_text_edit_disabled(&mut self, handle: &TextEditHandle, disabled: bool) {
        let text_edit_inner = &mut self.text_edits[handle.i as usize].0;
        text_edit_inner.disabled = disabled;
        if disabled {
            if let Some(AnyBox::TextEdit(e)) = self.focused {
                if e == handle.i {
                    self.get_full_text_edit(&handle).text_box.reset_selection();
                    self.focused = None;
                }
            }
        }

    }

    /// Returns whether any text was changed in the last frame.
    pub fn get_text_changed(&self) -> bool {
        self.shared.text_changed
    }

    /// Get a mutable reference to a text box wrapped with its style.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    /// 
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_box_mut(&mut self, handle: &TextBoxHandle) -> TextBoxMut {
        let text_box_inner = &mut self.text_boxes[handle.i as usize];
        TextBoxMut { inner: text_box_inner, shared: &mut self.shared }
    }

    /// If we did it this way, we could return a real reference to the fake struct, instead of the fake struct. It would be a much better interface. We could get rid of the TextBox/TextBoxMut split and use normal mutability of reference, just like if we were returning a real reference to a real inner struct.
    /// 
    /// you could do this without unsafe if there was a `self lifetime, but it would still be a bit weird.
    ///
    /// For the non-mut version of this, you'd need a way to have an unbounded number of these fake structs, either in a slab or something or on the heap or on a temporary allocator. That would be crazy though. 
    /// 
    /// I guess the real solution would be TextBox having some sort of reference semantics, where you can make &TextBox and &mut TextBox work like the current TextBox and TextBoxMut. And there would be no such thing as a owned TextBox, it would automatically own the two references, just like a reference automatically owns its pointer. 
    #[allow(dead_code)]
    pub(crate) fn get_text_box_mut_but_epic<'a>(&'a mut self, handle: &TextBoxHandle) -> &'a mut TextBoxMut<'a> {
        // SAFETY: since this function borrows the whole Text struct, there's no way to call any functions that would invalidate the references.
        unsafe {
            let text_box_inner = &mut self.text_boxes[handle.i as usize];

            // Fill the slot with pointers to fields in `self.
            self.slot_for_text_box_mut = Some(TextBoxMut {
                inner: std::mem::transmute(text_box_inner),
                shared: std::mem::transmute(&mut self.shared),
            });

            // Transmute the TextBoxMut in the slot, which is 'static, to have the lifetime of 'self.
            // It really does have the lifetime of 'self. We just don't have a way to express it.
            if let Some(slot) = &mut self.slot_for_text_box_mut {
                let result: &'a mut TextBoxMut<'a> = std::mem::transmute(slot);
                return result;
            } else {
                unreachable!()
            }
        }

    }

    /// Get a mutable reference to a text box wrapped with its style.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    /// 
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_box(&self, handle: &TextBoxHandle) -> TextBox {
        let text_box_inner = &self.text_boxes[handle.i as usize];
        TextBox { inner: text_box_inner, shared: &self.shared }
    }

    pub(crate) fn get_full_text_box(&mut self, i: &TextBoxHandle) -> TextBoxMut<'_> {
        get_full_text_box_free_function(&mut self.text_boxes, &mut self.shared, i)
    }

    pub(crate) fn get_full_text_edit(&mut self, i: &TextEditHandle) -> TextEditMut<'_> {
        get_full_text_edit_free_function(&mut self.text_edits, &mut self.shared, i)
    }

    /// Add a scroll animation for a text edit
    pub(crate) fn add_scroll_animation(&mut self, handle: TextEditHandle, start_offset: f32, target_offset: f32, duration: std::time::Duration, direction: ScrollDirection) {
        // Remove any existing animation for this handle and direction
        self.scroll_animations.retain(|anim| !(anim.handle.i == handle.i && anim.direction == direction));
        
        let animation = ScrollAnimation {
            start_offset,
            target_offset,
            start_time: std::time::Instant::now(),
            duration,
            direction,
            handle,
        };
        
        self.scroll_animations.push(animation);
    }

    /// Get the maximum remaining animation duration, if any animations are running.
    fn get_max_animation_duration(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut max_remaining = Duration::ZERO;
        let mut has_animations = false;
        
        for animation in &self.scroll_animations {
            let elapsed = now.duration_since(animation.start_time);
            if elapsed < animation.duration {
                let remaining = animation.duration - elapsed;
                if remaining > max_remaining {
                    max_remaining = remaining;
                }
                has_animations = true;
            }
        }
        
        if has_animations {
            Some(max_remaining)
        } else {
            None
        }
    }

    /// Update smooth scrolling animations for all text edits automatically.
    /// Returns true if any text edit animations were updated and require redrawing.
    fn update_smooth_scrolling(&mut self) -> bool {
        let mut needs_redraw = false;
        
        // Update all active animations
        let mut i = 0;
        while i < self.scroll_animations.len() {
            let animation = &self.scroll_animations[i];
            let handle = TextEditHandle { i: animation.handle.i };
            
            if let Some((_text_edit_inner, text_box_inner)) = self.text_edits.get_mut(handle.i as usize) {
                let current_offset = animation.get_current_offset();
                
                match animation.direction {
                    ScrollDirection::Horizontal => {
                        text_box_inner.scroll_offset.0 = current_offset;
                    }
                    ScrollDirection::Vertical => {
                        text_box_inner.scroll_offset.1 = current_offset;
                    }
                }
                
                if animation.is_finished() {
                    self.scroll_animations.remove(i);
                    // Don't increment i since we removed an element
                } else {
                    i += 1;
                }
                
                needs_redraw = true;
            } else {
                // Text edit doesn't exist anymore, remove the animation
                self.scroll_animations.remove(i);
            }
        }
        
        needs_redraw
    }

    fn handle_text_edit_scroll_event(&mut self, handle: &TextEditHandle, event: &WindowEvent, _window: &Window) -> bool {
        let mut did_scroll = false;

        if let WindowEvent::MouseWheel { delta, .. } = event {
            let shift_held = self.input_state.modifiers.state().shift_key();
            
            if let Some((text_edit_inner, text_box_inner)) = self.text_edits.get_mut(handle.i as usize) {
                if text_edit_inner.single_line {
                    // Single-line horizontal scrolling
                    let scroll_amount = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => {
                            if shift_held {
                                y * 120.0
                            } else {
                                x * 120.0
                            }
                        },
                        winit::event::MouseScrollDelta::PixelDelta(pos) => {
                            if shift_held {
                                pos.y as f32 
                            } else {
                                pos.x as f32
                            }
                        },
                    };
                    
                    if scroll_amount != 0.0 {
                        let current_scroll = text_box_inner.scroll_offset.0;
                        let target_scroll = current_scroll - scroll_amount;
                        
                        let total_text_width = text_box_inner.layout.full_width();
                        let text_width = text_box_inner.max_advance;
                        let max_scroll = (total_text_width - text_width).max(0.0).round() + crate::text_edit::CURSOR_WIDTH;
                        let clamped_target = target_scroll.clamp(0.0, max_scroll).round();
                        
                        if (clamped_target - current_scroll).abs() > 0.1 {
                            if should_use_animation(delta, shift_held) {
                                let animation_duration = std::time::Duration::from_millis(200);
                                self.add_scroll_animation(handle.clone(), current_scroll, clamped_target, animation_duration, ScrollDirection::Horizontal);
                            } else {
                                text_box_inner.scroll_offset.0 = clamped_target;
                            }
                            did_scroll = true;
                        }
                    }
                } else {
                    // Multi-line vertical scrolling
                    let scroll_amount = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_x, y) => y * 120.0,
                        winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                    };
                    
                    if scroll_amount != 0.0 {
                        let current_scroll = text_box_inner.scroll_offset.1;
                        let target_scroll = current_scroll - scroll_amount;
                        
                        let total_text_height = text_box_inner.layout.height();
                        let text_height = text_box_inner.height;
                        let max_scroll = (total_text_height - text_height).max(0.0).round();
                        let clamped_target = target_scroll.clamp(0.0, max_scroll).round();
                        
                        if (clamped_target - current_scroll).abs() > 0.1 {
                            if should_use_animation(delta, true) {
                                let animation_duration = std::time::Duration::from_millis(200);
                                self.add_scroll_animation(handle.clone(), current_scroll, clamped_target, animation_duration, ScrollDirection::Vertical);
                            } else {
                                text_box_inner.scroll_offset.1 = clamped_target;
                            }
                            did_scroll = true;
                        }
                    }
                }
            }
        }

        did_scroll
    }

    // result: (currently blinked, changed).
    pub(crate) fn cursor_blinked_out(&mut self, update: bool) -> (bool, bool) {
        if let Some(start_time) = self.cursor_blink_start {
            let elapsed = Instant::now().duration_since(start_time);
            let blink_period = Duration::from_millis(CURSOR_BLINK_TIME_MILLIS);
            let blinked_out = (elapsed.as_millis() / blink_period.as_millis()) % 2 == 0;
            let changed = blinked_out != self.cursor_currently_blinked_out;
            if update {
                self.cursor_currently_blinked_out = blinked_out;
            }
            return (blinked_out, changed);
        } else {
            (false, false)
        }
    }

    /// Returns the duration until the next cursor blink state change.
    /// 
    /// Returns `None` if cursor blinking should not be blinking.
    pub fn time_until_next_cursor_blink(&self) -> Option<Duration> {
        if let Some(start_time) = self.cursor_blink_start {
            let elapsed = Instant::now().duration_since(start_time);
            let blink_period = Duration::from_millis(CURSOR_BLINK_TIME_MILLIS);
            let elapsed_in_current_cycle = elapsed.as_millis() % blink_period.as_millis();
            let time_until_next_blink = blink_period.as_millis() - elapsed_in_current_cycle;
            Some(Duration::from_millis(time_until_next_blink as u64))
        } else {
            None
        }
    }

    // If the cursor needs to be blinking, reset it. Otherwise, stop it.
    fn reset_cursor_blink(&mut self) {
        if let Some(AnyBox::TextEdit(i)) = self.focused {
            let handle = TextEditHandle { i: i as u32 };
            let text_edit = self.get_full_text_edit(&handle);
            if text_edit.text_box.selection().is_collapsed() {
                
                self.cursor_blink_start = Some(Instant::now());
                self.decorations_changed = true;
                
                if let Some(timer) = &self.cursor_blink_timer {
                    timer.start_waker();
                }

                return;             
            }
        }

        self.cursor_blink_start = None;
        if let Some(timer) = &self.cursor_blink_timer {
            timer.stop_waker();
        }

    }
    
    pub fn set_focus<T: IntoAnyBox>(&mut self, handle: &T) {
        let handle: AnyBox = (*handle).into_anybox();
        self.refocus(Some(handle));
    }
    
    /// Update the AccessKit node ID mapping for a text box
    #[cfg(feature = "accessibility")]
    pub fn set_text_box_accesskit_id(&mut self, handle: &TextBoxHandle, accesskit_id: NodeId) {
        let any_box = handle.into_anybox();
        self.accesskit_id_to_text_handle_map.insert(accesskit_id, any_box);
        self.get_text_box_mut(handle).set_accesskit_id(accesskit_id);
    }
    
    /// Update the AccessKit node ID mapping for a text edit
    #[cfg(feature = "accessibility")]
    pub fn set_text_edit_accesskit_id(&mut self, handle: &TextEditHandle, accesskit_id: NodeId) {
        let any_box = handle.into_anybox();
        self.accesskit_id_to_text_handle_map.insert(accesskit_id, any_box);
        self.get_text_edit_mut(handle).set_accesskit_id(accesskit_id);
    }
    
    /// Get the text handle for a given AccessKit node ID
    #[cfg(feature = "accessibility")]
    pub(crate) fn get_text_handle_by_accesskit_id(&self, node_id: NodeId) -> Option<AnyBox> {
        self.accesskit_id_to_text_handle_map.get(&node_id).copied()
    }

    #[cfg(feature = "accessibility")]
    pub fn set_focus_by_accesskit_id(&mut self, focus: NodeId) {
        if let Some(focused_text_handle) = self.get_text_handle_by_accesskit_id(focus) {
            self.set_focus(&focused_text_handle);
        }
    }
    
    /// Set a custom node ID generator function for accessibility
    /// 
    /// The generator function will be called whenever a new accessibility node ID is needed.
    /// This allows you to control the node ID allocation strategy.
    /// 
    /// # Example
    /// ```ignore
    /// use accesskit::NodeId;
    /// 
    /// fn my_generator() -> NodeId {
    ///     // Your custom logic here
    ///     NodeId(42)
    /// }
    /// 
    /// text.set_node_id_generator(my_generator);
    /// ```
    #[cfg(feature = "accessibility")]
    pub fn set_node_id_generator(&mut self, generator: fn() -> NodeId) {
        self.shared.node_id_generator = generator;
    }

    pub fn focus(&self) -> Option<AnyBox> {
        self.focused
    }
    
    /// Get the AccessKit node ID of the currently focused text element
    /// 
    /// Returns `None` if no element is focused or if the focused element doesn't have an AccessKit ID.
    #[cfg(feature = "accessibility")]
    pub fn focused_accesskit_id(&self) -> Option<NodeId> {
        if let Some(focused) = self.focused {
            match focused {
                AnyBox::TextEdit(i) => {
                    if let Some((_text_edit, text_box)) = self.text_edits.get(i as usize) {
                        text_box.accesskit_id
                    } else {
                        None
                    }
                }
                AnyBox::TextBox(i) => {
                    if let Some(text_box) = self.text_boxes.get(i as usize) {
                        text_box.accesskit_id
                    } else {
                        None
                    }
                }
            }
        } else {
            None
        }
    }

    #[cfg(feature = "accessibility")]
    pub fn accesskit_update(&mut self, current_focused_node_id: NodeId, root_node_id: NodeId) -> Option<(TreeUpdate, FocusUpdate)> {
        // For some reason, every update that we send must specify the focus again.
        // If something else changed it, we'd end up overriding it.
        // So we have to ask for the current one from outside and fill that in, in case that nothing happened.
        // According to the TreeUpdate docs, we should also set focus = root_node_id when text boxes are defocused.
        // However, this means that the focus actually goes to the whole window, Windows Narrator says the name of the window, and the blue box covers the whole window. I don't really see this behavior anywhere else.

        let mut focus_update = FocusUpdate::Unchanged;
        
        let focus_update_is_fresh = self.shared.current_event_number == self.shared.accesskit_focus_update.1;
        if focus_update_is_fresh {
            if let Some(focus) = self.shared.accesskit_focus_update.0 {
                focus_update = FocusUpdate::FocusedNewNode(focus);
            } else {
                focus_update = FocusUpdate::Defocused;
            }
        }

        if focus_update == FocusUpdate::Unchanged && self.shared.accesskit_tree_update.nodes.is_empty() {
            return None;
        }

        let focus_value = match focus_update {
            FocusUpdate::FocusedNewNode(node_id) => node_id,
            FocusUpdate::Defocused => root_node_id,
            FocusUpdate::Unchanged => current_focused_node_id,
        };

        self.shared.accesskit_tree_update.focus = focus_value;
        let res = self.shared.accesskit_tree_update.clone();

        // Reset to an empty update.
        self.shared.accesskit_tree_update = TreeUpdate {
            nodes: Vec::new(),
            tree: None,
            focus: NodeId(0), // This doesn't matter.
        };
        return Some((res, focus_update));
    }

    #[cfg(feature = "accessibility")]
    fn push_ak_update_for_focused(&mut self, focused: AnyBox) {
        match focused {
            AnyBox::TextEdit(i) => {
                let handle = TextEditHandle { i };
                let mut text_edit = self.get_text_edit_mut(&handle);
                text_edit.push_accesskit_update_to_self();
            },
            AnyBox::TextBox(i) => {
                let handle = TextBoxHandle { i };
                let mut text_box = self.get_text_box_mut(&handle);
                text_box.push_accesskit_update_to_self();
            },
        }
    }
}

// I love partial borrows!
pub(crate) fn get_full_text_box_free_function<'a>(
    text_boxes: &'a mut Slab<TextBoxInner>,
    shared: &'a mut Shared,
    i: &TextBoxHandle,
) -> TextBoxMut<'a> {
    let text_box_inner = &mut text_boxes[i.i as usize];
    TextBoxMut { inner: text_box_inner, shared }
}

pub(crate) fn get_full_text_edit_free_function<'a>(
    text_edits: &'a mut Slab<(TextEditInner, TextBoxInner)>,
    shared: &'a mut Shared,
    i: &TextEditHandle,
) -> TextEditMut<'a> {
    let (text_edit_inner, text_box_inner) = text_edits.get_mut(i.i as usize).unwrap();
    let text_box = TextBoxMut { inner: text_box_inner, shared };
    TextEditMut { inner: text_edit_inner, text_box }
}

pub(crate) fn get_full_text_edit_free_function_but_for_iterating<'a>(
    text_edit: (&'a mut TextEditInner, &'a mut TextBoxInner),
    shared: &'a mut Shared,
) -> TextEditMut<'a> {
    let (text_edit_inner, text_box_inner) = text_edit;
    let text_box = TextBoxMut { inner: text_box_inner, shared };
    TextEditMut { inner: text_edit_inner, text_box }
}

pub(crate) fn get_full_text_box_free_function_but_for_iterating<'a>(
    text_box_inner: &'a mut TextBoxInner,
    shared: &'a mut Shared,
) -> TextBoxMut<'a> {
    TextBoxMut { inner: text_box_inner, shared }
}

/// Move quads in atlas pages to reflect new scroll position
fn move_quads_for_scroll(text_renderer: &mut TextRenderer, quad_storage: &mut QuadStorage, current_offset: (f32, f32)) {
    let delta_x = current_offset.0 - quad_storage.last_offset.0;
    let delta_y = current_offset.1 - quad_storage.last_offset.1;

    // Use rounded deltas
    let delta_x_rounded = delta_x.round();
    let delta_y_rounded = delta_y.round();

    // Move quads across all atlas pages
    for page_range in &quad_storage.pages {
        match page_range.page_type {
            AtlasPageType::Mask => {
                if let Some(page) = text_renderer.text_renderer.mask_atlas_pages.get_mut(page_range.page_index as usize) {
                    for quad_index in page_range.quad_start..page_range.quad_end {
                        if let Some(quad) = page.quads.get_mut(quad_index as usize) {
                            quad.pos[0] = quad.pos[0] - delta_x_rounded as i32;
                            quad.pos[1] = quad.pos[1] - delta_y_rounded as i32;
                        }
                    }
                }
            },
            AtlasPageType::Color => {
                if let Some(page) = text_renderer.text_renderer.color_atlas_pages.get_mut(page_range.page_index as usize) {
                    for quad_index in page_range.quad_start..page_range.quad_end {
                        if let Some(quad) = page.quads.get_mut(quad_index as usize) {
                            quad.pos[0] = quad.pos[0] - delta_x_rounded as i32;
                            quad.pos[1] = quad.pos[1] - delta_y_rounded as i32;
                        }
                    }
                }
            },
        }
    }

    // Update stored offset
    quad_storage.last_offset.0 += delta_x_rounded;
    quad_storage.last_offset.1 += delta_y_rounded;
}

/// Result of handling a window event, providing hints to the user about what actions they should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventResult {
    /// Whether the event was consumed by a text widget
    pub consumed: bool,
    /// Whether the user should rerender the text
    pub need_rerender: bool,
    /// Whether the event loop should be set to wake up after a duration. This is set when the cursor should blink.
    /// 
    /// Using this value is not recommended: instead, you can set up the winit event loop to wake automatically when needed. See the `smart_event_loop.rs` example for how to do this.
    pub wake_up_event_loop_in: Option<Duration>,
}
impl EventResult {
    fn nothing() -> EventResult {
        EventResult {
            consumed: false,
            need_rerender: false,
            wake_up_event_loop_in: None,
        }
    }
}

// todo: get this from system settings.
const CURSOR_BLINK_TIME_MILLIS: u64 = 500;

#[derive(Debug)]
enum WakerCommand {
    Start,
    Stop,
    Exit,
}

pub(crate) struct CursorBlinkWaker {
    command_sender: mpsc::Sender<WakerCommand>,
}

impl Drop for CursorBlinkWaker {
    fn drop(&mut self) {
        // Signal the thread to exit
        let _ = self.command_sender.send(WakerCommand::Exit);
    }
}

impl CursorBlinkWaker {
    fn new(window: Weak<Window>) -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        
        thread::spawn(move || {
            let mut is_running = false;
            
            loop {
                if is_running {
                    // While running, wait for either a command or timeout
                    match command_receiver.recv_timeout(Duration::from_millis(CURSOR_BLINK_TIME_MILLIS)) {
                        Ok(WakerCommand::Start) => {}
                        Ok(WakerCommand::Stop) => is_running = false,
                        Ok(WakerCommand::Exit) => return,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Timeout occurred, request redraw directly
                            if let Some(window) = window.upgrade() {
                                window.request_redraw();
                            } else {
                                // Window has been dropped, exit thread
                                return;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    // While stopped, wait indefinitely for a command
                    match command_receiver.recv() {
                        Ok(WakerCommand::Start) => is_running = true,
                        Ok(WakerCommand::Stop) => {}
                        Ok(WakerCommand::Exit) => return,
                        Err(_) => return,
                    }
                }
            }
        });
        
        Self {
            command_sender,
        }
    }
        
    fn start_waker(&self) {
        let _ = self.command_sender.send(WakerCommand::Start);
    }
    
    fn stop_waker(&self) {
        let _ = self.command_sender.send(WakerCommand::Stop);
    }
}