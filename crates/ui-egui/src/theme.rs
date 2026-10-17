//! Design tokens (the shared craft-ui System / Light / Dark / High contrast appearances), fonts
//! and egui style. The look follows treeforjs/craftworks DESIGN.md, not Illustrator's.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};
use serde::{Deserialize, Serialize};

/// The interface appearance: the shared craft-ui choices every craft app offers (System, Light,
/// Dark, High contrast). Illustrator's four brightness levels are gone; older settings and
/// scripts that name them read as the nearest of these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brightness {
    /// Follow the operating system's light or dark setting.
    #[default]
    System,
    #[serde(alias = "MediumLight")]
    Light,
    #[serde(alias = "MediumDark")]
    Dark,
    HighContrast,
}

impl Brightness {
    pub const ALL: [Brightness; 4] = [Brightness::System, Brightness::Light, Brightness::Dark, Brightness::HighContrast];
    pub fn label(self) -> &'static str {
        self.craft().label()
    }
    pub fn id(self) -> &'static str {
        self.craft().id()
    }
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(b) = Self::ALL.into_iter().find(|b| b.id().eq_ignore_ascii_case(s) || b.label().eq_ignore_ascii_case(s)) {
            return Some(b);
        }
        // The retired Illustrator brightness levels.
        match s.to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
            "mediumdark" => Some(Brightness::Dark),
            "mediumlight" => Some(Brightness::Light),
            _ => craft_ui::Appearance::parse(s).map(Self::from_craft),
        }
    }
    fn craft(self) -> craft_ui::Appearance {
        match self {
            Brightness::System => craft_ui::Appearance::System,
            Brightness::Light => craft_ui::Appearance::Light,
            Brightness::Dark => craft_ui::Appearance::Dark,
            Brightness::HighContrast => craft_ui::Appearance::HighContrast,
        }
    }
    fn from_craft(a: craft_ui::Appearance) -> Self {
        match a {
            craft_ui::Appearance::System => Brightness::System,
            craft_ui::Appearance::Light => Brightness::Light,
            craft_ui::Appearance::Dark => Brightness::Dark,
            craft_ui::Appearance::HighContrast => Brightness::HighContrast,
        }
    }
    /// The colour mode this choice draws in, given the desktop's theme (None: unknown, dark).
    pub fn mode(self, system: Option<egui::Theme>) -> craft_ui::Mode {
        self.craft().resolve(system, true)
    }
}

/// All colours and metrics the UI uses. Never hard-code a colour in a widget.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub app_bar: Color32,
    pub panel: Color32,
    pub panel_darker: Color32,
    pub input: Color32,
    pub input_border: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub hover: Color32,
    pub tool_active: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub row_selected: Color32,
    pub pasteboard: Color32,
    pub ruler: Color32,
    pub ruler_tick: Color32,
    pub smart_guide: Color32,
    pub guide: Color32,
    pub measure_bg: Color32,
    pub button: Color32,
    pub radius: u8,
    /// Field values, button labels, link labels.
    pub text_strong: Color32,
    /// Panel tab strips, doc-tab bar, dock header.
    pub tab_strip: Color32,
    /// Outline of secondary (outlined) buttons.
    pub button_border: Color32,
    /// Default Layer 1 selection colour and selected-anchor fill.
    pub selection: Color32,
    pub anchor_selected: Color32,
    /// Active item text in tool flyouts.
    pub flyout_active: Color32,
    /// Hint-bar / tab-bar heights etc. scale with this base font size.
    pub font_size: f32,
    /// Window Close button (custom title bar) when hovered, and its glyph there.
    pub caption_close: Color32,
    pub caption_close_text: Color32,
    /// What preview images mark (Flattener Preview highlights).
    pub highlight: Color32,
    /// The bleed outline around artboards (Document Setup → Bleed).
    pub bleed: Color32,
    /// Error marks (a missing linked file).
    pub error: Color32,
    /// Warning marks (a modified linked file).
    pub warning: Color32,
}

impl Tokens {
    /// The tokens for `b`, with System drawn dark (see [`Tokens::for_mode`] for the resolved mode).
    pub fn for_brightness(b: Brightness) -> Self {
        Self::for_mode(b.mode(None))
    }

    /// VectorCraft's tokens for one of the shared craft-ui modes, so it matches the other apps.
    pub fn for_mode(mode: craft_ui::Mode) -> Self {
        let hex = |s: u32| Color32::from_rgb((s >> 16) as u8, (s >> 8) as u8, s as u8);
        let c = craft_ui::Tokens::new(mode, craft_ui::Density::Compact);
        Tokens {
            dark: mode != craft_ui::Mode::Light,
            app_bar: c.bg,
            panel: c.surface,
            panel_darker: c.bg,
            input: c.field,
            input_border: c.border,
            border: c.border,
            divider: c.divider,
            text: c.text,
            text_dim: c.text_muted,
            text_disabled: c.text_disabled,
            icon: c.icon,
            hover: c.hover,
            tool_active: c.pressed,
            accent: c.accent,
            accent_strong: c.accent_hover,
            row_selected: c.accent_soft,
            pasteboard: c.bg,
            ruler: c.surface,
            ruler_tick: c.text_faint,
            // Canvas overlays keep their own hues: they must read on any artwork.
            smart_guide: hex(0xff3dfc),
            guide: hex(0x4affff),
            measure_bg: Color32::from_rgba_unmultiplied(92, 92, 92, 235),
            button: c.surface_raised,
            radius: craft_ui::tokens::radius::SM,
            text_strong: c.text,
            tab_strip: c.bg,
            button_border: c.border,
            selection: hex(0x4f80ff),
            anchor_selected: hex(0x3d82ff),
            flyout_active: c.accent_text,
            font_size: 13.0,
            caption_close: hex(0xc42b1c),
            caption_close_text: Color32::WHITE,
            highlight: hex(0xff3030),
            bleed: hex(0xf03030),
            error: c.danger,
            warning: c.warning,
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::NULL)).unwrap_or_else(|| Tokens::for_brightness(Brightness::Dark))
    }
    pub fn cr(&self) -> CornerRadius {
        CornerRadius::same(self.radius)
    }
}

pub const FONT_UI: &str = "ui";
pub const FONT_UI_SEMIBOLD: &str = "ui-semibold";
pub const FONT_MONO: &str = "mono";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_UI_SEMIBOLD.into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_MONO.into()))
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "SourceSans3", include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf"));
    add(&mut fonts, "SourceSans3-Semibold", include_bytes!("../../../assets/fonts/SourceSans3-Semibold.ttf"));
    add(&mut fonts, "JetBrainsMono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));
    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["SourceSans3".to_string()];
    prop.extend(fallback.clone());
    fonts.families.insert(FontFamily::Proportional, prop);
    let mut semi = vec!["SourceSans3-Semibold".to_string()];
    semi.extend(fallback.clone());
    fonts.families.insert(FontFamily::Name(FONT_UI_SEMIBOLD.into()), semi);
    let mut ui = vec!["SourceSans3".to_string()];
    ui.extend(fallback.clone());
    fonts.families.insert(FontFamily::Name(FONT_UI.into()), ui);
    let mut mono = vec!["JetBrainsMono".to_string()];
    mono.extend(fallback);
    fonts.families.insert(FontFamily::Name(FONT_MONO.into()), mono);
    add_craft_fonts(&mut fonts);
    ctx.set_fonts(fonts);
}

/// The egui name of a craft-fonts face.
pub(crate) fn craft_font_name(f: &vectorcraft_text::CraftFont) -> String {
    format!("craft-fonts {} {}", f.family, f.style)
}

/// The Japanese craft-fonts faces (when built with `CRAFT_FONTS_DIR`) as fallbacks at the end of
/// every family, after the app's own fonts and before the installed fonts `ui_fonts` adds: BIZ
/// UDPGothic first (bold first in the semibold family), then the Mincho faces.
fn add_craft_fonts(fonts: &mut FontDefinitions) {
    for f in vectorcraft_text::CRAFT_FONTS.iter().filter(|f| f.is_japanese()) {
        fonts.font_data.insert(craft_font_name(f), Arc::new(FontData::from_static(f.bytes)));
    }
    for (family, stack) in fonts.families.iter_mut() {
        let bold = *family == FontFamily::Name(FONT_UI_SEMIBOLD.into());
        stack.extend(vectorcraft_text::craft_fonts::japanese_ui_fonts(bold).into_iter().map(craft_font_name));
    }
}

/// Apply tokens to egui's global style.
pub fn apply(ctx: &egui::Context, b: Brightness) {
    let t = Tokens::for_mode(b.mode(ctx.system_theme()));
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, t));
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.input;
    v.faint_bg_color = t.panel_darker;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::same(craft_ui::tokens::radius::XL);
    v.menu_corner_radius = CornerRadius::same(craft_ui::tokens::radius::LG);
    v.selection.bg_fill = t.accent.gamma_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.text);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 60 }) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if t.dark { 140 } else { 70 }) };
    let w = &mut v.widgets;
    for (wv, fill) in [
        (&mut w.noninteractive, t.panel),
        (&mut w.inactive, t.button),
        (&mut w.hovered, t.hover),
        (&mut w.active, t.tool_active),
        (&mut w.open, t.hover),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.corner_radius = t.cr();
        wv.fg_stroke = Stroke::new(1.0, t.text);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    // An outline so unchecked boxes and idle fields read on light surfaces too.
    w.inactive.bg_stroke = Stroke::new(1.0, t.input_border);
    w.hovered.bg_stroke = Stroke::new(1.0, t.input_border);
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 5.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size = egui::vec2(24.0, 24.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.slider_width = 120.0;
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, FontId::new(15.0, FontFamily::Name(FONT_UI_SEMIBOLD.into()))),
            (TextStyle::Monospace, FontId::new(11.5, FontFamily::Name(FONT_MONO.into()))),
        ]
        .into();
        s.animation_time = 0.08;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_roundtrip() {
        for b in Brightness::ALL {
            assert_eq!(Brightness::parse(b.id()), Some(b));
            assert_eq!(Brightness::parse(b.label()), Some(b));
        }
        assert!(!Tokens::for_brightness(Brightness::Light).dark);
        // Older settings and scripts that name Illustrator's levels still load.
        assert_eq!(Brightness::parse("mediumDark"), Some(Brightness::Dark));
        assert_eq!(Brightness::parse("Medium Light"), Some(Brightness::Light));
        assert_eq!(serde_json::from_str::<Brightness>("\"MediumDark\"").ok(), Some(Brightness::Dark));
        assert_eq!(Brightness::parse("purple"), None);
    }

    /// The colours are craft-ui's, so VectorCraft matches the other craft apps, and System
    /// follows the desktop.
    #[test]
    fn tokens_come_from_craft_ui() {
        let light = craft_ui::Tokens::new(craft_ui::Mode::Light, craft_ui::Density::Compact);
        let t = Tokens::for_brightness(Brightness::Light);
        assert_eq!((t.panel, t.accent, t.text), (light.surface, light.accent, light.text));
        assert_eq!(Brightness::System.mode(Some(egui::Theme::Light)), craft_ui::Mode::Light);
        assert_eq!(Brightness::System.mode(None), craft_ui::Mode::Dark);
        assert!(Tokens::for_brightness(Brightness::HighContrast).dark);
    }
}
