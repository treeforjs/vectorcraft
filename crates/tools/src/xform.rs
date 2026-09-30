//! Transform and utility tools (rotate/reflect/scale/shear, free transform, eyedropper, gradient,
//! artboard, magic wand, lasso, measure).

use crate::Tool;

/// Create a transform/utility tool by id (None = not one of ours).
pub fn create(_id: &str) -> Option<Box<dyn Tool>> {
    None
}
