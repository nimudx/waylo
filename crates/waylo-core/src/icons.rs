//! Location of application icons in the system themes (paths only; decoding belongs to
//! `waylo-render`).

use std::path::{Path, PathBuf};

/// Looks up an icon by name in the theme (large PNG first, then SVG).
pub fn resolve(name: &str) -> Option<PathBuf> {
    if name.starts_with('/') {
        return Path::new(name).exists().then(|| PathBuf::from(name));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let roots = [
        "/usr/share/icons/hicolor".to_string(),
        format!("{home}/.local/share/icons/hicolor"),
        "/var/lib/flatpak/exports/share/icons/hicolor".to_string(),
        "/usr/share/icons/Adwaita".to_string(),
    ];
    for size in ["256x256", "512x512", "128x128", "96x96", "64x64", "48x48"] {
        for root in &roots {
            let p = PathBuf::from(format!("{root}/{size}/apps/{name}.png"));
            if p.exists() {
                return Some(p);
            }
        }
    }
    for root in &roots {
        let p = PathBuf::from(format!("{root}/scalable/apps/{name}.svg"));
        if p.exists() {
            return Some(p);
        }
    }
    // Breeze lays things out as apps/<size>/name.svg
    for theme in ["breeze", "breeze-dark", "AdwaitaLegacy"] {
        for size in ["64", "48", "96", "128", "256", "32"] {
            for ext in ["svg", "png"] {
                let p = PathBuf::from(format!("/usr/share/icons/{theme}/apps/{size}/{name}.{ext}"));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    // Generic theme icons (devices, categories...)
    for group in ["devices", "categories", "places", "mimetypes"] {
        for root in ["/usr/share/icons/Adwaita", "/usr/share/icons/AdwaitaLegacy"] {
            let p = PathBuf::from(format!("{root}/scalable/{group}/{name}.svg"));
            if p.exists() {
                return Some(p);
            }
            let p = PathBuf::from(format!("{root}/64x64/{group}/{name}.png"));
            if p.exists() {
                return Some(p);
            }
        }
    }
    for ext in ["png", "svg"] {
        let p = PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}
