//! Drawing and path-editing tools (pencil, curvature, anchor tools, scissors, knife, line family…).

use crate::Tool;

/// Create a drawing tool by id (None = not one of ours).
pub fn create(_id: &str) -> Option<Box<dyn Tool>> {
    None
}
