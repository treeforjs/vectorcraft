//! Document ruler coordinates. These are view settings, not artwork transforms.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vectorcraft_geom::Point;

use crate::Artboard;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RulerMode {
    /// Preserve the document-wide coordinate convention of older native files.
    #[default]
    Global,
    Artboard,
}

/// Stored with native files, restored separately from undoable artwork.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulerSettings {
    pub mode: RulerMode,
    /// Absolute document point; `None` means the first artboard's top-left corner.
    pub global_origin: Option<Point>,
    /// Offsets from each artboard's top-left corner, keyed by its stable id.
    pub artboard_origins: BTreeMap<u32, Point>,
}

impl RulerSettings {
    /// Bound imported and agent-supplied origins before ruler arithmetic.
    pub fn valid_point(point: Point) -> bool {
        point.x.is_finite() && point.y.is_finite() && point.x.abs() <= 1e9 && point.y.abs() <= 1e9
    }

    pub fn origin(&self, boards: &[Artboard], active: usize) -> Point {
        let board = match self.mode {
            RulerMode::Global => boards.first(),
            RulerMode::Artboard => boards.get(active).or_else(|| boards.first()),
        };
        let base = board.map_or(Point::ZERO, |b| Point::new(b.rect.x0, b.rect.y0));
        let base = if Self::valid_point(base) { base } else { Point::ZERO };
        let origin = match self.mode {
            RulerMode::Global => self.global_origin.unwrap_or(base),
            RulerMode::Artboard => {
                let offset = board.and_then(|b| self.artboard_origins.get(&b.id)).copied().unwrap_or(Point::ZERO);
                Point::new(base.x + offset.x, base.y + offset.y)
            }
        };
        if Self::valid_point(origin) { origin } else { base }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::Rect;

    fn boards() -> Vec<Artboard> {
        vec![
            Artboard { id: 3, name: "A".into(), rect: Rect::new(10.0, 20.0, 110.0, 120.0), show_center_mark: false, show_cross_hairs: false },
            Artboard { id: 8, name: "B".into(), rect: Rect::new(200.0, -40.0, 300.0, 60.0), show_center_mark: false, show_cross_hairs: false },
        ]
    }

    #[test]
    fn origins_follow_mode_and_stable_artboard_ids() {
        let mut b = boards();
        let mut r = RulerSettings::default();
        assert_eq!(r.origin(&b, 1), Point::new(10.0, 20.0));
        r.mode = RulerMode::Artboard;
        r.artboard_origins.insert(8, Point::new(5.0, 7.0));
        assert_eq!(r.origin(&b, 1), Point::new(205.0, -33.0));
        b.swap(0, 1);
        assert_eq!(r.origin(&b, 0), Point::new(205.0, -33.0));
        b[0].rect = Rect::new(300.0, 0.0, 400.0, 100.0);
        assert_eq!(r.origin(&b, 0), Point::new(305.0, 7.0));
    }

    #[test]
    fn hostile_origins_fall_back_and_legacy_settings_deserialize() {
        let mut r: RulerSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(r, RulerSettings::default());
        for p in [Point::new(f64::NAN, 2.0), Point::new(f64::INFINITY, 0.0), Point::new(1e300, -1e300)] {
            r.global_origin = Some(p);
            assert_eq!(r.origin(&boards(), 0), Point::new(10.0, 20.0));
        }
        assert_eq!(r.origin(&[], usize::MAX), Point::ZERO);
    }
}
