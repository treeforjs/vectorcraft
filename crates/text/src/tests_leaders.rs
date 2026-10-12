//! Leaders are additional paint, with the tab's style, never synthetic source text.

use super::*;
use kurbo::Shape;
use vectorcraft_doc::{CharStyle, PathEffect, TabAlign, TabStop, TextKind, TextRun};
use vectorcraft_geom::PathData;

fn db() -> &'static FontDb {
    static DB: std::sync::OnceLock<FontDb> = std::sync::OnceLock::new();
    DB.get_or_init(|| FontDb::with_font_dirs(vec![]))
}

fn text(pattern: &str, align: TabAlign) -> TextObject {
    let mut t = TextObject::point(Point::ZERO, "Item\t12.5", CharStyle { size: 18.0, ..CharStyle::default() });
    t.para.tabs = vec![TabStop { position: 200.0, align, leader: pattern.into(), align_on: '.' }];
    t
}

#[test]
fn tab_leaders_repeat_without_changing_text_glyphs_carets_or_alignment() {
    for align in [TabAlign::Left, TabAlign::Center, TabAlign::Right, TabAlign::Decimal] {
        let t = text(". - ", align);
        let plain = layout(db(), &text("", align));
        let l = layout(db(), &t);
        assert_eq!(t.plain_text(), "Item\t12.5");
        assert_eq!(l.glyphs.len(), plain.glyphs.len());
        for (a, b) in l.glyphs.iter().zip(&plain.glyphs) {
            assert_eq!((a.byte, a.len, a.origin, a.advance, &a.outline), (b.byte, b.len, b.origin, b.advance, &b.outline));
        }
        assert_eq!(l.tab_leaders.len(), 1);
        let tab = l.glyphs.iter().find(|g| g.byte == 4).unwrap();
        let bounds = l.tab_leaders[0].outline.bounding_box();
        assert!(bounds.width() > tab.advance * 0.7, "leader fills the gap: {bounds:?}, {tab:?}");
        assert!(bounds.x0 >= tab.origin.x - 1.0 && bounds.x1 <= tab.origin.x + tab.advance + 1.0);
        for byte in [0, 4, 5, 9] {
            assert_eq!(caret_position(&l, byte), caret_position(&plain, byte));
        }
        assert_eq!(decorations(&l, db(), &t).len(), 1);
    }
}

#[test]
fn tab_leader_outlines_use_the_tab_run_style() {
    let mut t = text(".", TabAlign::Left);
    let small = CharStyle { size: 12.0, ..CharStyle::default() };
    let large = CharStyle { size: 30.0, baseline_shift: 8.0, h_scale: 140.0, ..small.clone() };
    t.runs = vec![TextRun::new("Item", small.clone()), TextRun::new("\t", large.clone()), TextRun::new("12.5", small)];
    let l = layout(db(), &t);
    assert_eq!(l.tab_leaders[0].run, 1);
    let reference = layout(db(), &TextObject::point(Point::ZERO, ".", large));
    let first = &reference.glyphs[0].outline;
    let tab = l.glyphs.iter().find(|g| g.byte == 4).unwrap();
    let translated = Affine::translate(tab.origin.to_vec2()) * first.clone();
    assert_eq!(&l.tab_leaders[0].outline.elements()[..translated.elements().len()], translated.elements());
}

#[test]
fn tab_leaders_follow_vertical_alignment_columns_and_vertical_type() {
    let mut t = text(". ", TabAlign::Left);
    t.kind = TextKind::Area { frame: PathData::from_bezpath(&Rect::new(0.0, 0.0, 500.0, 120.0).to_path(0.1)) };
    t.runs[0].text = "Item\t12.5\nItem\t8\nItem\t9\nItem\t3\nItem\t4\nItem\t5".into();
    t.area.columns = 2;
    t.area.gutter = 20.0;
    let top = layout(db(), &t);
    assert!(!top.tab_leaders.is_empty());
    t.area.vertical_align = VerticalAlign::Bottom;
    let bottom = layout(db(), &t);
    assert_eq!(top.tab_leaders.len(), bottom.tab_leaders.len());
    for (a, b) in top.tab_leaders.iter().zip(&bottom.tab_leaders) {
        let shift = bottom.lines[b.line].baseline - top.lines[a.line].baseline;
        let expected = Affine::translate((0.0, shift)) * a.outline.clone();
        assert_eq!(b.outline, expected);
    }
    let mut vertical = text(".", TabAlign::Left);
    vertical.vertical = true;
    let l = layout(db(), &vertical);
    assert!(l.vertical && l.tab_leaders.len() == 1);
    let bounds = l.tab_leaders[0].outline.bounding_box();
    assert!(bounds.height() > bounds.width() * 4.0, "leader follows the column: {bounds:?}");
    assert!(!decorations(&l, db(), &vertical).is_empty());
}

#[test]
fn tab_leaders_follow_path_effects_without_adding_source_glyphs() {
    let path = PathData::from_bezpath(&BezPath::from_svg("M 0 100 C 100 -40 300 180 500 0").unwrap());
    for effect in [PathEffect::Rainbow, PathEffect::Skew, PathEffect::Ribbon3d, PathEffect::StairStep, PathEffect::Gravity] {
        let mut t = text(". ", TabAlign::Left);
        t.kind = TextKind::OnPath { path: path.clone(), start: 0.0, end: None };
        t.path_effect = effect;
        let l = layout(db(), &t);
        assert_eq!(l.glyphs.len(), t.plain_text().chars().count());
        assert!(l.on_path && !l.overflow);
        assert_eq!(l.tab_leaders.len(), 1);
        assert!(l.tab_leaders[0].outline.bounding_box().width() > 30.0);
        assert!(!decorations(&l, db(), &t).is_empty());
    }
}

#[test]
fn tab_leader_work_is_bounded_for_hostile_patterns_and_gaps() {
    let mut t = text("\t\n\r", TabAlign::Left);
    assert!(layout(db(), &t).tab_leaders.is_empty());
    t.para.tabs[0].leader = ".".into();
    t.para.tabs[0].position = 1e100;
    let l = layout(db(), &t);
    assert!(l.overflow && l.leader_limit);
    assert!(l.leader_glyphs <= 65_536 && l.leader_elements <= 2_000_000);
    t.runs[0].style.h_scale = 0.0;
    assert!(layout(db(), &t).tab_leaders.is_empty());
}
