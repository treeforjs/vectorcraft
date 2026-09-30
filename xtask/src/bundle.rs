//! `cargo xtask bundle`: build the release app and wrap it as `dist/DrawCraft.app` (macOS), with an
//! icon rendered by DrawCraft's own renderer.

use std::path::Path;
use std::process::Command;

use drawcraft_color::{Color, Gradient, GradientKind, GradientPaint, GradientStop, Paint};
use drawcraft_doc::{Appearance, Document, Node};
use drawcraft_geom::{Anchor, PathData, Point, Rect, SubPath, shapes};
use drawcraft_render::Renderer;

/// The app icon: a warm-gradient rounded square with a white pen nib (our own artwork).
pub fn icon_png(size: u32) -> Vec<u8> {
    let s = 1024.0;
    let mut d = Document::new(s, s);
    let l = d.layers[0].id;
    let grad = GradientPaint::new(Gradient {
        kind: GradientKind::Linear,
        stops: vec![
            GradientStop { offset: 0.0, color: Color::from_hex("#ff9a3c").unwrap(), opacity: 1.0, midpoint: 0.5 },
            GradientStop { offset: 1.0, color: Color::from_hex("#d4145a").unwrap(), opacity: 1.0, midpoint: 0.5 },
        ],
    });
    let mut gp = grad;
    gp.angle = -45.0;
    let bg = d.alloc_id();
    d.insert(Some(l), 9, Node::path(bg, shapes::rounded_rectangle(Rect::new(100.0, 100.0, 924.0, 924.0), 185.0), Appearance::basic(Paint::Gradient(Box::new(gp)), Paint::None, 0.0))).unwrap();
    // Pen nib.
    let c = Point::new(512.0, 520.0);
    let nib = PathData::single(SubPath::new(
        vec![
            Anchor::corner(Point::new(c.x, c.y + 250.0)),
            Anchor::with_handles(Point::new(c.x - 170.0, c.y - 20.0), Point::new(c.x - 150.0, c.y + 80.0), Point::new(c.x - 170.0, c.y - 90.0)),
            Anchor::corner(Point::new(c.x - 95.0, c.y - 230.0)),
            Anchor::corner(Point::new(c.x + 95.0, c.y - 230.0)),
            Anchor::with_handles(Point::new(c.x + 170.0, c.y - 20.0), Point::new(c.x + 170.0, c.y - 90.0), Point::new(c.x + 150.0, c.y + 80.0)),
        ],
        true,
    ));
    let n = d.alloc_id();
    d.insert(Some(l), 9, Node::path(n, nib, Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0))).unwrap();
    let hole = d.alloc_id();
    d.insert(Some(l), 9, Node::path(hole, shapes::ellipse(Rect::from_center_size(Point::new(c.x, c.y - 30.0), (74.0, 74.0))), Appearance::basic(Paint::Gradient(Box::new(GradientPaint::new(Gradient::default()))), Paint::None, 0.0))).unwrap();
    d.node_mut(hole).unwrap().appearance = Appearance::basic(Paint::solid(Color::from_hex("#e8573f").unwrap()), Paint::None, 0.0);
    let slit = d.alloc_id();
    d.insert(Some(l), 9, Node::path(slit, shapes::rectangle(Rect::new(c.x - 9.0, c.y + 5.0, c.x + 9.0, c.y + 245.0)), Appearance::basic(Paint::solid(Color::from_hex("#e0457a").unwrap()), Paint::None, 0.0))).unwrap();
    Renderer::new().render_region(&d, Rect::new(0.0, 0.0, s, s), size as f64 / s, false).to_png()
}

pub fn run(root: &Path) -> Result<(), String> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())).current_dir(root).args(["build", "--release", "-p", "drawcraft", "-p", "drawcraft-cli"]).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("release build failed".into());
    }
    let target = std::env::var("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| root.join("target"));
    let app = root.join("dist/DrawCraft.app/Contents");
    let _ = std::fs::remove_dir_all(root.join("dist/DrawCraft.app"));
    std::fs::create_dir_all(app.join("MacOS")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app.join("Resources")).map_err(|e| e.to_string())?;
    std::fs::copy(target.join("release/drawcraft"), app.join("MacOS/DrawCraft")).map_err(|e| format!("copy app: {e}"))?;
    std::fs::copy(target.join("release/drawcraft-cli"), app.join("MacOS/drawcraft-cli")).map_err(|e| format!("copy cli: {e}"))?;
    // Icon set → .icns via iconutil (macOS build tool).
    let iconset = root.join("dist/DrawCraft.iconset");
    let _ = std::fs::remove_dir_all(&iconset);
    std::fs::create_dir_all(&iconset).map_err(|e| e.to_string())?;
    for (sz, name) in [(16, "16x16"), (32, "16x16@2x"), (32, "32x32"), (64, "32x32@2x"), (128, "128x128"), (256, "128x128@2x"), (256, "256x256"), (512, "256x256@2x"), (512, "512x512"), (1024, "512x512@2x")] {
        std::fs::write(iconset.join(format!("icon_{name}.png")), icon_png(sz)).map_err(|e| e.to_string())?;
    }
    std::fs::write(root.join("dist/DrawCraft-icon.png"), icon_png(1024)).map_err(|e| e.to_string())?;
    let ok = Command::new("iconutil").args(["-c", "icns"]).arg(&iconset).arg("-o").arg(app.join("Resources/DrawCraft.icns")).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        eprintln!("warning: iconutil failed or missing; bundle has no icon");
    }
    let version = env!("CARGO_PKG_VERSION");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>DrawCraft</string>
<key>CFBundleDisplayName</key><string>DrawCraft</string>
<key>CFBundleIdentifier</key><string>ai.storyteller.drawcraft</string>
<key>CFBundleExecutable</key><string>DrawCraft</string>
<key>CFBundleIconFile</key><string>DrawCraft</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>CFBundleDocumentTypes</key><array>
 <dict><key>CFBundleTypeName</key><string>DrawCraft Document</string><key>CFBundleTypeExtensions</key><array><string>drawcraft</string></array><key>CFBundleTypeRole</key><string>Editor</string></dict>
 <dict><key>CFBundleTypeName</key><string>SVG</string><key>CFBundleTypeExtensions</key><array><string>svg</string></array><key>CFBundleTypeRole</key><string>Editor</string></dict>
 <dict><key>CFBundleTypeName</key><string>PDF / Illustrator</string><key>CFBundleTypeExtensions</key><array><string>pdf</string><string>ai</string></array><key>CFBundleTypeRole</key><string>Viewer</string></dict>
</array>
</dict></plist>
"#
    );
    std::fs::write(app.join("Info.plist"), plist).map_err(|e| e.to_string())?;
    println!("built {}", root.join("dist/DrawCraft.app").display());
    Ok(())
}
