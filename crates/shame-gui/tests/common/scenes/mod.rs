//! Built-in demo scenes — the single source of truth for the visual test
//! scenarios. Both snapshot tests and runnable examples build their apps from
//! here, so a scene change is verified by the golden comparison and can be
//! played by hand without redefining anything.

pub mod blend;
pub mod form;
pub mod push_constant;
pub mod rects;
pub mod text;
pub mod viewport_rect;

#[allow(unused_imports)]
pub use blend::blend_scene;
#[allow(unused_imports)]
pub use form::form_scene;
#[allow(unused_imports)]
pub use push_constant::push_constant_scene;
#[allow(unused_imports)]
pub use rects::rects_scene;
#[allow(unused_imports)]
pub use text::{text_scene, text_scene_with};
#[allow(unused_imports)]
pub use viewport_rect::viewport_rect_scene;
