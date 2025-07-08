//! Parley2 is an experimental high level text library based on Parley.
//! 
//! The goal is to allow any winit/wgpu program to have full-featured text and text editing with minimal integration effort.
//! 
//! Most GUI programs or toolkits, games, etc. don't have any advanced requirements for text: they just want basic text boxes that "work the same way as everywhere else" (browsers, native operating system GUIs, etc.).
//! 
//! If all that is available is a relatively low level library such as Parley, all these projects will have to do a large amount of repeated work, needlessly raising the barrier of entry to GUI programming.
//! 
//! # Limitations
//! 
//! This library is not ready for use. It currently does not reach its own goal of "full featured text".
//! 
//! - Accessibility is supported in Parley itself but not in Parley2, because of my personal lack of familiarity with the subject.
//! 
//! - Parley2 currently uses the built-in Swash CPU rasterizer and a basic homemade atlas renderer to actually show the text on screen. The performance is acceptable but it is not as good as it could be. There is also a questionable bit of unsafe code to circumvent some performance problems in Swash. This might eventually be fixed by switching to the new "Vello Hybrid" renderer.
//! 
//! - Parley itself has some limitations, but they will be probably fixed soon.
//! 
//! 
//! # Usage
//! 
//! See the `basic.rs` example in the repository to see how the library can be used.
//! 
//! The two main structs are:
//! - [`Text`] - manages collections of text boxes and styles
//! - [`TextRenderer`] - renders the text on the GPU 
//! 
//! ## Imperative Mode
//! 
//! The main way to use the library is imperative with a handle-based system:
//! 
//! ```rust
//! let mut text = Text::new();
//! 
//! // Add text boxes and get handles
//! let handle = text.add_text_box("Hello".to_string(), (10.0, 10.0), (200.0, 50.0), 0.0);
//! let edit_handle = text.add_text_edit("Type here".to_string(), (10.0, 70.0), (200.0, 30.0), 0.0);
//! 
//! // Use handles to access and modify the boxes
//! text.get_text_box_mut(&handle).set_style(&my_style);
//! text.get_text_box_mut(&handle).raw_text_mut().push_str("... World");
//! text.get_text_box_mut(&handle).set_hidden(true);
//! 
//! // Manually remove text boxes
//! text.remove_text_box(handle);
//! text.remove_text_edit(edit_handle);
//! ```
//! 
//! This interface is ideal for retained-mode GUI libraries, but declarative GUI libraries that diff their node trees can still use the imperative interface, calling the `Text::remove_*` functions when the nodes holding the handles are removed.
//! 
//! Handles can't be `Clone`d or constructed manually, so they are unique references that can never be "dangling".
//! 
//! [`Text`] uses slabs internally, so `get_text_box_mut()` and all similar functions are basically as fast as an array lookup. There is no hashing involved.
//! 
//! ## Declarative Mode
//! 
//! There is an optional declarative interface for hiding and removing text boxes. For hiding text boxes declaratively:
//! 
//! ```ignore
//! // Each frame, advance an internal frame counter,
//! // and implicitly mark all text boxes as "outdated"
//! text.advance_frame_and_hide_boxes();
//! 
//! // "Refresh" only the nodes that should remain visible
//! for node in current_nodes {
//!     text.refresh_text_box(&node.text_box_handle);
//! }
//! 
//! // Text boxes that were not refreshed will be remain hidden,
//! // and they will be skipped when rendering or handling events.
//! ```
//! 
//! There's also an experimental function for removing text boxes declaratively: [`Text::remove_old_nodes()`].
//! 
//! This library was written for use in Keru. Keru is a declarative library that diffs node trees, so it uses imperative-mode calls to remove widgets. However, it uses the declarative interface for hiding text boxes that need to be kept hidden in the background.
//! 
//! ## Interaction
//! 
//! Text boxes and text edit boxes are fully interactive. In simple situations, this requires a single function call: [`Text::handle_event()`]. This function takes a `winit::WindowEvent` and updates all the text boxes accordingly.
//! 
//! As great as this sounds, sometimes text boxes are occluded by other objects, such as an opaque panel. In this case, handling a mouse click event requires information that the [`Text`] struct doesn't have, so the integration needs to be a bit more complex. The process is this:
//! 
//! - Run `let topmost_text_box = `[`Text::find_topmost_text_box()`] to find out which text box *would* have received the event, if it wasn't for other objects.
//! - Run some custom code to find out which other object *would* have received the event, if it wasn't for text boxes.
//! - Compare the depth of the two candidates. For the text box, use [`Text::get_text_box_depth()`].
//! - If the text box is on top, run [`Text::handle_event_with_topmost()`]`(Some(topmost_text_box))`, which will handle the event normally, but avoid looking for the topmost box again.
//! - If the text box, is occluded, run [`Text::handle_event_with_topmost()`]`(None)`.
//! 
//! For any `winit::WindowEvent` other than a `winit::WindowEvent::MouseInput`, this process can be skipped, and you can just call [`Text::handle_event()`].
//! 
//! The `occlusion.rs` example shows how this works.


mod setup;
pub use setup::*;

mod text_renderer;
pub use text_renderer::*;

mod text;
pub use text::*;

mod text_box;
pub use text_box::*;

mod text_edit;
pub use text_edit::*;

pub use parley::TextStyle as ParleyTextStyle;

/// Text style.
/// 
/// To use it, first add a `TextStyle` into a [`Text`] with [`Text::add_style()`], and get a [`StyleHandle`] back. Then, use [`TextBox::set_style()`] to make a text box use the style.
pub type TextStyle2 = ParleyTextStyle<'static, ColorBrush>;

use bytemuck::{Pod, Zeroable};
use etagere::euclid::{Size2D, UnknownUnit};
use etagere::{size2, Allocation, BucketedAtlasAllocator};
use lru::LruCache;
use rustc_hash::FxHasher;
use swash::zeno::{Format, Vector};

use wgpu::*;

use image::{GrayImage, Luma, Rgba, RgbaImage};
use parley::{
    Glyph, GlyphRun,
    Layout, PositionedLayoutItem,
};
use std::borrow::Cow;
use std::hash::BuildHasherDefault;
use std::mem;
use std::num::NonZeroU64;
use swash::scale::image::{Content, Image};
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::{FontRef, GlyphId};
use wgpu::{
    MultisampleState, Texture, TextureFormat,
    TextureUsages, TextureViewDescriptor,
};
use swash::zeno::Placement;
