//! `cargo xtask bundle`: build the release app and wrap it as `dist/VectorCraft.app` (macOS), with the
//! committed app icon `assets/app-icon/vectorcraft.icns` (regenerate it with `packaging/icons.sh`).

use std::path::Path;
use std::process::Command;

pub fn run(root: &Path) -> Result<(), String> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args(["build", "--release", "-p", "vectorcraft", "-p", "vectorcraft-cli"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("release build failed".into());
    }
    let target = std::env::var("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| root.join("target"));
    let app = root.join("dist/VectorCraft.app/Contents");
    let _ = std::fs::remove_dir_all(root.join("dist/VectorCraft.app"));
    std::fs::create_dir_all(app.join("MacOS")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app.join("Resources")).map_err(|e| e.to_string())?;
    std::fs::copy(target.join("release/vectorcraft"), app.join("MacOS/VectorCraft")).map_err(|e| format!("copy app: {e}"))?;
    std::fs::copy(target.join("release/vectorcraft-cli"), app.join("MacOS/vectorcraft-cli")).map_err(|e| format!("copy cli: {e}"))?;
    std::fs::copy(root.join("assets/app-icon/vectorcraft.icns"), app.join("Resources/VectorCraft.icns")).map_err(|e| format!("copy icon: {e}"))?;
    let version = env!("CARGO_PKG_VERSION");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>VectorCraft</string>
<key>CFBundleDisplayName</key><string>VectorCraft</string>
<key>CFBundleIdentifier</key><string>ai.storyteller.vectorcraft</string>
<key>CFBundleExecutable</key><string>VectorCraft</string>
<key>CFBundleIconFile</key><string>VectorCraft</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>CFBundleDocumentTypes</key><array>
 <dict><key>CFBundleTypeName</key><string>VectorCraft Document</string><key>CFBundleTypeExtensions</key><array><string>vectorcraft</string></array><key>CFBundleTypeRole</key><string>Editor</string></dict>
 <dict><key>CFBundleTypeName</key><string>SVG</string><key>CFBundleTypeExtensions</key><array><string>svg</string></array><key>CFBundleTypeRole</key><string>Editor</string></dict>
 <dict><key>CFBundleTypeName</key><string>PDF / Illustrator</string><key>CFBundleTypeExtensions</key><array><string>pdf</string><string>ai</string></array><key>CFBundleTypeRole</key><string>Viewer</string></dict>
</array>
</dict></plist>
"#
    );
    std::fs::write(app.join("Info.plist"), plist).map_err(|e| e.to_string())?;
    println!("built {}", root.join("dist/VectorCraft.app").display());
    Ok(())
}
