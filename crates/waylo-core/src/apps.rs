//! Installed applications (`.desktop` files), their usage and how to launch them.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs};

#[derive(Debug, Clone)]
pub struct App {
    /// Name of the .desktop file (stable identifier).
    pub id: String,
    pub name: String,
    exec: String,
    /// Value of `Icon=` (theme name or absolute path).
    pub icon: Option<String>,
    terminal: bool,
    /// Lowercase text to search in: name, generic name, keywords.
    search: String,
    name_lower: String,
}

fn home() -> String {
    env::var("HOME").unwrap_or_default()
}

/// .desktop directories, from highest to lowest priority (the user's wins).
fn dirs() -> Vec<PathBuf> {
    let home = home();
    let mut list = vec![
        PathBuf::from(format!("{home}/.local/share/applications")),
        PathBuf::from(format!("{home}/.local/share/flatpak/exports/share/applications")),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    for d in env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into()).split(':') {
        if !d.is_empty() {
            list.push(PathBuf::from(d).join("applications"));
        }
    }
    list
}

/// Strips the field codes (%u, %F...) from Exec; `%%` becomes `%`.
fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if let Some('%') = chars.peek() {
            chars.next();
            out.push('%');
        } else {
            chars.next(); // discard the code
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse(id: &str, text: &str, lang: &str) -> Option<App> {
    let mut in_entry = false;
    let mut f: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if in_entry {
                break; // actions and other sections are not of interest
            }
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            f.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    if f.get("Type").map(String::as_str) != Some("Application") {
        return None;
    }
    let flag = |k: &str| f.get(k).is_some_and(|v| v == "true");
    if flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    let shows = |k: &str| f.get(k).map(|v| v.split(';').any(|e| e.eq_ignore_ascii_case("Hyprland")));
    if f.get("OnlyShowIn").is_some_and(|v| !v.is_empty()) && shows("OnlyShowIn") != Some(true) {
        return None;
    }
    if shows("NotShowIn") == Some(true) {
        return None;
    }
    let exec = clean_exec(f.get("Exec")?);
    if exec.is_empty() {
        return None;
    }
    let name = f
        .get(&format!("Name[{lang}]"))
        .or_else(|| f.get("Name"))
        .cloned()
        .filter(|n| !n.is_empty())?;
    let mut search = name.to_lowercase();
    for key in ["GenericName", "Keywords", "Comment"] {
        if let Some(v) = f.get(key) {
            search.push(' ');
            search.push_str(&v.to_lowercase());
        }
    }
    Some(App { id: id.to_string(), name_lower: name.to_lowercase(), name, exec, terminal: flag("Terminal"), icon: f.get("Icon").cloned().filter(|i| !i.is_empty()), search })
}

/// All visible applications, without duplicates.
pub fn load() -> Vec<App> {
    let lang: String = env::var("LANG").unwrap_or_default().chars().take(2).collect();
    let mut seen: HashMap<String, App> = HashMap::new();
    for dir in dirs() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            if seen.contains_key(&id) {
                continue; // already seen in a higher-priority directory
            }
            if let Some(app) = fs::read_to_string(&path).ok().and_then(|t| parse(&id, &t, &lang)) {
                seen.insert(id, app);
            }
        }
    }
    seen.into_values().collect()
}

impl App {
    /// Path of the icon file, if it exists in any theme.
    pub fn icon_path(&self) -> Option<PathBuf> {
        self.icon.as_deref().and_then(crate::icons::resolve)
    }

    /// Match score for the search (lower = better). None if it does not match.
    pub fn rank(&self, query: &str) -> Option<u8> {
        if query.is_empty() {
            return Some(0);
        }
        if self.name_lower.starts_with(query) {
            Some(0)
        } else if self.name_lower.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(query)) {
            Some(1)
        } else if self.name_lower.contains(query) {
            Some(2)
        } else if self.search.contains(query) {
            Some(3)
        } else {
            None
        }
    }
}

// ───────────────────────── Usage (for sorting) ─────────────────────────

fn usage_path() -> PathBuf {
    crate::paths::config_file("launcher.toml").unwrap_or_else(|| PathBuf::from("launcher.toml"))
}

/// Number of times each application was opened.
pub fn usage() -> BTreeMap<String, u32> {
    let read = |path: PathBuf| -> Option<BTreeMap<String, u32>> {
        let text = fs::read_to_string(path).ok()?;
        toml::from_str::<BTreeMap<String, BTreeMap<String, u32>>>(&text).ok()?.remove("counts")
    };
    // If Waylo has no counters yet, reuse those of the previous launcher (xdisplay).
    read(usage_path())
        .or_else(|| read(PathBuf::from(format!("{}/.config/xdisplay/launcher.toml", home()))))
        .unwrap_or_default()
}

fn bump(id: &str) {
    let mut counts = usage();
    *counts.entry(id.to_string()).or_insert(0) += 1;
    let table = BTreeMap::from([("counts".to_string(), counts)]);
    if let Ok(text) = toml::to_string_pretty(&table) {
        let path = usage_path();
        let _ = fs::create_dir_all(path.parent().unwrap());
        let _ = fs::write(path, text);
    }
}

// ───────────────────────── Launch ─────────────────────────

/// Opens the application detached from us (it stays alive even if the launcher closes).
pub fn launch(app: &App) -> Result<(), String> {
    let mut cmd = Command::new("setsid");
    cmd.arg("-f");
    if app.terminal {
        let term = env::var("TERMINAL").unwrap_or_else(|_| "kitty".into());
        cmd.args([term.as_str(), "-e", "sh", "-c", &app.exec]);
    } else {
        cmd.args(["sh", "-c", &format!("exec {}", app.exec)]);
    }
    cmd.current_dir(home()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().map_err(|e| format!("no pude abrir {}: {e}", app.name))?;
    bump(&app.id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_sin_codigos_de_campo() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(clean_exec("env FOO=1 app --file %F  --x"), "env FOO=1 app --file --x");
        assert_eq!(clean_exec("sh -c 'echo 100%%'"), "sh -c 'echo 100%'");
    }

    #[test]
    fn ranking_de_busqueda() {
        let app = parse(
            "x.desktop",
            "[Desktop Entry]\nType=Application\nName=Visual Studio Code\nExec=code %F\nKeywords=editor;\n",
            "en",
        )
        .unwrap();
        assert_eq!(app.rank("vis"), Some(0));
        assert_eq!(app.rank("stu"), Some(1));
        assert_eq!(app.rank("sual"), Some(2));
        assert_eq!(app.rank("editor"), Some(3));
        assert_eq!(app.rank("zzz"), None);
    }

    #[test]
    fn oculta_apps_no_visibles() {
        assert!(parse("a", "[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n", "en").is_none());
        assert!(parse("a", "[Desktop Entry]\nType=Application\nName=A\nExec=a\nOnlyShowIn=GNOME;\n", "en").is_none());
        assert!(parse("a", "[Desktop Entry]\nType=Application\nName=A\nExec=a\nOnlyShowIn=Hyprland;\n", "en").is_some());
    }
}
