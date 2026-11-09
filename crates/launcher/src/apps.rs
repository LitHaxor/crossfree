use std::path::{Path, PathBuf};

use crate::icons::{self, Artwork};

/// A program installed in a prefix, found through its Start Menu shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppShortcut {
    /// Shortcut name without its extension.
    pub name: String,
    /// The shortcut: a `.lnk` file, or an Internet shortcut (`.url`), which
    /// is how Steam adds its games.
    pub path: PathBuf,
    /// The program's icon as an `.ico` file, if it has one.
    pub icon: Option<PathBuf>,
    /// Library artwork, for Steam games.
    pub artwork: Artwork,
    /// The Windows program a `.lnk` shortcut runs (`C:\…\steam.exe`): Wine
    /// names the program's process after it.
    pub program: Option<String>,
    /// The Windows folder a Steam game is installed in: its processes run
    /// from there.
    pub install_dir: Option<String>,
}

/// How deep to look below a Start Menu `Programs` folder.
const MAX_DEPTH: usize = 6;

/// Start Menu shortcuts in a prefix: the all-users menu and every user's
/// own, without uninstallers and web links (see
/// [`is_web_link`](icons::is_web_link)), de-duplicated by name and sorted.
///
/// With `icon_cache`, each shortcut's icon is extracted into that directory
/// (normally [`icon_cache_dir`](crate::icon_cache_dir)); without it, no
/// shortcut has an icon.
pub fn shortcuts(prefix: &Path, icon_cache: Option<&Path>) -> Vec<AppShortcut> {
    let drive_c = prefix.join("drive_c");
    let menu = Path::new("Microsoft/Windows/Start Menu/Programs");
    let mut roots = vec![drive_c.join("ProgramData").join(menu)];
    if let Ok(users) = std::fs::read_dir(drive_c.join("users")) {
        let mut users: Vec<PathBuf> = users.flatten().map(|e| e.path()).collect();
        users.sort();
        roots.extend(users.into_iter().map(|u| u.join("AppData/Roaming").join(menu)));
    }

    let mut found = Vec::new();
    for root in roots {
        walk(&root, 0, &mut found);
    }
    found.sort_by(|a: &AppShortcut, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    for app in &mut found {
        app.icon = icon_cache.and_then(|cache| icons::icon_for_shortcut(&app.path, prefix, cache));
        app.artwork = icons::artwork(&app.path, prefix);
        app.program = icons::shortcut_program(&app.path);
        app.install_dir = app.artwork.steam_app_id.and_then(|id| icons::install_dir(prefix, id));
    }
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<AppShortcut>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            walk(&path, depth + 1, found);
            continue;
        }
        let is_shortcut =
            path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk") || e.eq_ignore_ascii_case("url"));
        let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        if is_shortcut && kind.is_file() && !name.to_lowercase().starts_with("uninstall") && !icons::is_web_link(&path)
        {
            found.push(AppShortcut {
                name,
                path,
                icon: None,
                artwork: Artwork::default(),
                program: None,
                install_dir: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::fixtures;
    use crate::test_support::TempDir;

    fn write(path: &Path, data: impl AsRef<[u8]>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    fn touch(path: &Path) {
        write(path, b"");
    }

    #[test]
    fn finds_start_menu_shortcuts() {
        let dir = TempDir::new("apps");
        let p = dir.path();
        assert!(shortcuts(p, None).is_empty());
        let common = p.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs");
        let user = p.join("drive_c/users/someone/AppData/Roaming/Microsoft/Windows/Start Menu/Programs");
        touch(&common.join("Steam/Steam.lnk"));
        touch(&common.join("Steam/Uninstall Steam.lnk"));
        // A web link, as Steam writes it: not a program.
        write(
            &common.join("Steam/Steam Support Center.url"),
            "[InternetShortcut]\r\nURL=http://support.steampowered.com/\r\n",
        );
        write(&common.join("Steam/Mail Us.URL"), "[InternetShortcut]\nurl=MAILTO:help@example.com\n");
        touch(&common.join("Steam/readme.txt"));
        touch(&user.join("Rise of Nations.LNK"));
        write(
            &user.join("Steam/Rise of Nations Extended Edition.url"),
            "[InternetShortcut]\r\nURL=steam://rungameid/287450\r\n",
        );
        touch(&user.join("Steam.lnk")); // same name as the all-users one
        let apps = shortcuts(p, None);
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Rise of Nations", "Rise of Nations Extended Edition", "Steam"]);
        assert!(apps.iter().all(|a| a.icon.is_none()));
        // Empty stub files have no icons either.
        let cache = dir.path().join("cache");
        assert!(shortcuts(p, Some(&cache)).iter().all(|a| a.icon.is_none()));
    }

    #[test]
    fn shortcuts_carry_their_icons() {
        let dir = TempDir::new("app-icons");
        let p = dir.path();
        let cache = p.join("cache");
        let menu = p.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs/Game");
        let images = [fixtures::image(5)];
        write(&p.join("drive_c/Games/game.exe"), fixtures::pe(true, &[(1, vec![(1, images[0].clone())])]));
        write(&menu.join("Game.lnk"), fixtures::lnk(Some(r"C:\Games\game.exe"), None, 0));
        write(&menu.join("Game Website.url"), "[InternetShortcut]\r\nURL=https://example.com/\r\n");
        write(&menu.join("Other Game.url"), "[InternetShortcut]\r\nURL=steam://rungameid/10\r\n");

        let apps = shortcuts(p, Some(&cache));
        assert_eq!(apps.len(), 2, "the website is left out");
        // What each runs: the link's program; a Steam game's folder needs
        // its manifest, which this prefix lacks.
        assert_eq!(apps[0].program.as_deref(), Some(r"C:\Games\game.exe"));
        assert_eq!((apps[1].program.as_deref(), apps[1].install_dir.as_deref()), (None, None));
        let icon = apps[0].icon.as_ref().expect("the game has an icon");
        assert_eq!(std::fs::read(icon).unwrap(), fixtures::ico(&images));
        assert_eq!(apps[1].icon, None);
        assert!(shortcuts(p, None).iter().all(|a| a.icon.is_none()));
    }
}
