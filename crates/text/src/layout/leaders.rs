//! Original tab-leader layout from the public Tabs workflow:
//! https://helpx.adobe.com/illustrator/desktop/measure-and-align/grids-and-guides/manage-tab-stops.html
//! Leaders repeat up to eight characters, using the tab character's formatting. Their paint is
//! separate from source glyphs so selecting, copying and exporting searchable text keeps one tab.

use super::*;

const MAX_LEADER_GLYPHS: usize = 65_536;
const MAX_LEADER_ELEMENTS: usize = 2_000_000;

impl Ctx<'_> {
    pub(super) fn emit_leader(&mut self, pattern: &str, tab: &SGlyph, gap: f64, line: usize, position: impl Fn(f64, f64) -> (Affine, Point, f64)) {
        if pattern.is_empty() || !gap.is_finite() || gap <= EPS || self.out.leader_limit {
            return;
        }
        // Native files can bypass the command parser. Do not shape control characters or an
        // arbitrarily long pattern, and never loop on a zero/negative advance.
        let pattern: String = pattern.chars().take(8).filter(|ch| !ch.is_control()).collect();
        if pattern.is_empty() {
            return;
        }
        let mut shaped = {
            let style = self.style_at(tab.byte);
            let bidi = BidiInfo::new(&pattern, Some(tab.level));
            let cx = Ctx {
                on_path: self.on_path,
                db: self.db,
                text: &pattern,
                runs: vec![(0..pattern.len(), style)],
                inlines: vec![None],
                default: CharStyle::default(),
                opts: self.opts,
                vertical: self.vertical,
                out: TextLayout::default(),
            };
            let glyphs = cx.shape_para(0..pattern.len(), Some(&bidi));
            visual_order(Some(&bidi).filter(|_| !self.vertical), 0, 0..pattern.len(), &glyphs)
                .into_iter()
                .filter_map(|i| glyphs.get(i).cloned())
                .collect::<Vec<_>>()
        };
        let width: f64 = shaped.iter().map(|g| g.adv).sum();
        if shaped.is_empty() || !width.is_finite() || width <= EPS || shaped.iter().any(|g| !g.adv.is_finite() || g.adv < 0.0) {
            return;
        }
        for g in &mut shaped {
            g.run = tab.run;
            g.byte = tab.byte;
            g.len = 0;
        }
        let mut outline = BezPath::new();
        let repeats = (gap / width).ceil().min(MAX_LEADER_GLYPHS as f64) as usize;
        'patterns: for repeat in 0..repeats {
            let mut offset = repeat as f64 * width;
            for g in &shaped {
                if offset + g.adv > gap + EPS {
                    break 'patterns;
                }
                if self.out.leader_glyphs >= MAX_LEADER_GLYPHS {
                    self.out.leader_limit = true;
                    break 'patterns;
                }
                let (pre, origin, angle) = position(offset, g.adv);
                // Reuse ordinary glyph placement for baseline shifts, rotation, scaling and
                // vertical orientation, then retain only its paint, never an extra caret.
                self.emit(g, pre, origin, angle, g.adv, line);
                if let Some(glyph) = self.out.glyphs.pop() {
                    let count = glyph.outline.elements().len();
                    if self.out.leader_elements.saturating_add(count) > MAX_LEADER_ELEMENTS {
                        self.out.leader_limit = true;
                        break 'patterns;
                    }
                    outline.extend(glyph.outline.iter());
                    self.out.leader_elements += count;
                }
                self.out.leader_glyphs += 1;
                offset += g.adv;
            }
        }
        self.out.leader_limit |= gap / width > MAX_LEADER_GLYPHS as f64;
        if !outline.elements().is_empty() {
            self.out.tab_leaders.push(TabLeader { run: tab.run, line, outline });
        }
    }
}
