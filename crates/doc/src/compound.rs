//! Live compound shapes (Pathfinder › Shape Modes with Alt, Make Compound Shape). The group keeps
//! its operands, editable, in an invisible first child; the second child is the generated result.
//! Each operand carries the shape mode it applies to the stack below it, so the result follows
//! any edit to the operands. Rendering and exporters only see ordinary groups and paths.
use serde::{Deserialize, Serialize};

pub const OPERANDS: &str = "Compound Shape Operands";
pub const RESULT: &str = "Compound Shape Result";

/// How an operand combines with everything below it in the compound shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapeMode {
    #[default]
    Add,
    Subtract,
    Intersect,
    Exclude,
}

impl ShapeMode {
    pub const ALL: [ShapeMode; 4] = [ShapeMode::Add, ShapeMode::Subtract, ShapeMode::Intersect, ShapeMode::Exclude];
    pub fn id(self) -> &'static str {
        match self {
            ShapeMode::Add => "add",
            ShapeMode::Subtract => "subtract",
            ShapeMode::Intersect => "intersect",
            ShapeMode::Exclude => "exclude",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == s)
    }
}

/// One mode per operand, back to front. The bottom operand's mode is ignored (it starts the stack).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompoundShape {
    pub modes: Vec<ShapeMode>,
}

impl CompoundShape {
    /// The mode operand `i` applies (Add for operands the list doesn't cover).
    pub fn mode(&self, i: usize) -> ShapeMode {
        self.modes.get(i).copied().unwrap_or_default()
    }
}
