//! Library artwork for Steam games.
//!
//! Steam's client caches the images its library shows for each game under
//! `appcache/librarycache`: a wide hero banner, the game's logo, a portrait
//! cover and a small header. A game's Start Menu entry is an Internet
//! shortcut to `steam://rungameid/<app id>`, which names the game.

use std::path::{Path, PathBuf};

use super::{ini_section, locate, read_small, text};

/// Largest registry file read.
const MAX_REGISTRY_BYTES: u64 = 64 << 20;
/// Where Steam's installer puts it, after the folder Steam records.
const DEFAULT_STEAM: [&str; 2] = [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"];

/// Images of one game; any of them may be missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Artwork {
    /// The game's Steam app ID.
    pub steam_app_id: Option<u32>,
    /// Wide background banner (`library_hero.jpg`).
    pub hero: Option<PathBuf>,
    /// The game's logo on a transparent background (`logo.png`).
    pub logo: Option<PathBuf>,
    /// Portrait cover (`library_600x900.jpg`).
    pub cover: Option<PathBuf>,
    /// Small horizontal banner (`header.jpg`).
    pub header: Option<PathBuf>,
}

/// The library artwork of the shortcut at `shortcut` in `prefix`: empty
/// unless it is an Internet shortcut that opens a Steam game.
pub(crate) fn artwork(shortcut: &Path, prefix: &Path) -> Artwork {
    let is_url = shortcut.extension().is_some_and(|e| e.eq_ignore_ascii_case("url"));
    let id = is_url.then(|| read_small(shortcut)).flatten().and_then(|data| steam_app_id(&text(&data)));
    let Some(id) = id else { return Artwork::default() };
    let mut art = Artwork { steam_app_id: Some(id), ..Artwork::default() };
    for steam in steam_dirs(prefix) {
        let cache = steam.join("appcache/librarycache");
        // Current Steam keeps a folder per app; older releases used
        // `<id>_<name>` files.
        let find = |name: &str| {
            [cache.join(id.to_string()).join(name), cache.join(format!("{id}_{name}"))]
                .into_iter()
                .find(|p| p.is_file())
        };
        art.hero = art.hero.or_else(|| find("library_hero.jpg"));
        art.logo = art.logo.or_else(|| find("logo.png"));
        art.cover = art.cover.or_else(|| find("library_600x900.jpg"));
        art.header = art.header.or_else(|| find("header.jpg"));
    }
    art
}

/// The Steam app an Internet shortcut opens: `URL=steam://rungameid/<id>`
/// or `steam://run/<id>`.
fn steam_app_id(text: &str) -> Option<u32> {
    let (_, url) = ini_section(text, "InternetShortcut").find(|(k, _)| k.eq_ignore_ascii_case("URL"))?;
    let rest = ["steam://rungameid/", "steam://run/"].iter().find_map(|scheme| {
        url.get(..scheme.len()).filter(|head| head.eq_ignore_ascii_case(scheme)).map(|_| &url[scheme.len()..])
    })?;
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok().filter(|&id| id != 0)
}

/// The folder the Steam game `app_id` is installed in, as a Windows path
/// (`C:\Program Files (x86)\Steam\steamapps\common\<game>`): its
/// `appmanifest_<id>.acf` names it, in Steam's own library or one listed in
/// `steamapps/libraryfolders.vdf`. The game's processes run from there.
pub(crate) fn install_dir(prefix: &Path, app_id: u32) -> Option<String> {
    let registered = registered_steam_path(prefix);
    for steam in registered.iter().map(String::as_str).chain(DEFAULT_STEAM) {
        let Some(dir) = locate(steam, prefix, prefix).filter(|d| d.is_dir()) else { continue };
        let mut libraries = vec![steam.to_owned()];
        if let Some(folders) = read_small(&dir.join("steamapps/libraryfolders.vdf")) {
            libraries.extend(vdf_values(&text(&folders), "path"));
        }
        // Steam records its own folder with `/` (`c:/program files
        // (x86)/steam`); Wine names processes with `\`.
        for library in libraries.iter().map(|l| l.replace('/', r"\")) {
            let Some(root) = locate(&library, prefix, prefix) else { continue };
            let manifest = root.join(format!("steamapps/appmanifest_{app_id}.acf"));
            let name = read_small(&manifest).and_then(|data| vdf_values(&text(&data), "installdir").into_iter().next());
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                let library = library.trim_end_matches('\\');
                return Some(format!(r"{library}\steamapps\common\{name}"));
            }
        }
    }
    None
}

/// Every value of `key` in Valve's KeyValues text, lines of `"key" "value"`
/// (key case ignored; `\\` is a backslash).
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let (name, value) = (quoted.next()?, quoted.next()?);
            name.eq_ignore_ascii_case(key).then(|| value.replace(r"\\", r"\"))
        })
        .collect()
}

/// Where Steam is installed in `prefix`: the `SteamPath` the Steam client
/// records in the registry, then its default folders.
fn steam_dirs(prefix: &Path) -> Vec<PathBuf> {
    let registered = registered_steam_path(prefix);
    registered
        .iter()
        .map(String::as_str)
        .chain(DEFAULT_STEAM)
        .filter_map(|path| locate(path, prefix, prefix))
        .filter(|path| path.is_dir())
        .collect()
}

/// `SteamPath` under `HKEY_CURRENT_USER\Software\Valve\Steam`, from the
/// prefix's `user.reg`.
fn registered_steam_path(prefix: &Path) -> Option<String> {
    let path = prefix.join("user.reg");
    if std::fs::metadata(&path).ok()?.len() > MAX_REGISTRY_BYTES {
        return None;
    }
    let data = std::fs::read(&path).ok()?;
    let mut in_key = false;
    for line in String::from_utf8_lossy(&data).lines() {
        if let Some(header) = line.strip_prefix('[') {
            // `[Software\\Valve\\Steam] <timestamp>`: backslashes are doubled.
            let key = header.split_once(']').map_or(header, |(key, _)| key);
            in_key = key.eq_ignore_ascii_case(r"Software\\Valve\\Steam");
            continue;
        }
        let Some((name, value)) = line.split_once('=').filter(|_| in_key) else { continue };
        if name.trim().eq_ignore_ascii_case("\"SteamPath\"") {
            let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
            return Some(value.replace(r"\\", r"\"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A game in Steam's own library, and one in a library Steam lists in
    /// `libraryfolders.vdf`.
    #[test]
    fn finds_where_games_are_installed() {
        let dir = crate::test_support::TempDir::new("steam-install");
        let prefix = dir.path();
        let steam = prefix.join("drive_c/Program Files (x86)/Steam");
        put(
            &steam.join("steamapps/appmanifest_287450.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"287450\"\n\t\"name\"\t\t\"Rise of Nations\"\n\t\"installdir\"\t\t\"Rise of Nations\"\n}\n",
        );
        put(
            &steam.join("steamapps/libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Games\\\\Library\"\n\t}\n}\n",
        );
        put(
            &prefix.join("drive_c/Games/Library/steamapps/appmanifest_10.acf"),
            "\"AppState\"\n{\n\t\"installdir\"\t\t\"Half-Life\"\n}\n",
        );
        assert_eq!(
            install_dir(prefix, 287450).as_deref(),
            Some(r"C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations")
        );
        assert_eq!(install_dir(prefix, 10).as_deref(), Some(r"C:\Games\Library\steamapps\common\Half-Life"));
        assert_eq!(install_dir(prefix, 99), None);

        // The folder Steam registers comes first, slashes as Wine names
        // processes.
        put(
            &prefix.join("user.reg"),
            "[Software\\\\Valve\\\\Steam] 1\n\"SteamPath\"=\"c:/program files (x86)/steam\"\n",
        );
        assert_eq!(
            install_dir(prefix, 287450).as_deref(),
            Some(r"c:\program files (x86)\steam\steamapps\common\Rise of Nations")
        );
    }

    #[test]
    fn reads_keyvalues() {
        let text = "\"a\"\n{\n\t\"InstallDir\"\t\t\"x\"\n\t\"path\" \"C:\\\\y\"\n\t\"path\"\n}";
        assert_eq!(vdf_values(text, "installdir"), ["x"]);
        assert_eq!(vdf_values(text, "path"), [r"C:\y"]);
        assert!(vdf_values(text, "missing").is_empty());
    }
    use crate::test_support::TempDir;

    fn write(path: &Path, data: impl AsRef<[u8]>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    fn url(id: &str) -> String {
        format!("[InternetShortcut]\r\nURL={id}\r\nICONFILE=C:\\x.ico\r\n")
    }

    #[test]
    fn reads_the_steam_app_id() {
        assert_eq!(steam_app_id(&url("steam://rungameid/287450")), Some(287450));
        assert_eq!(steam_app_id(&url("STEAM://RUN/10/en")), Some(10));
        assert_eq!(steam_app_id(&url("steam://rungameid/287450?x=1")), Some(287450));
        assert_eq!(steam_app_id(&url("http://support.steampowered.com/")), None);
        assert_eq!(steam_app_id(&url("steam://rungameid/")), None);
        assert_eq!(steam_app_id(&url("steam://rungameid/0")), None);
        // Non-Steam games added to Steam get 64-bit IDs and no library art.
        assert_eq!(steam_app_id(&url("steam://rungameid/12345678901234567890")), None);
        assert_eq!(steam_app_id("[Other]\nURL=steam://rungameid/5\n"), None);
    }

    #[test]
    fn reads_steam_path_from_the_registry() {
        let dir = TempDir::new("steam-reg");
        assert_eq!(registered_steam_path(dir.path()), None);
        write(
            &dir.path().join("user.reg"),
            "WINE REGISTRY Version 2\n\n[Software\\\\Valve\\\\Steam\\\\Apps\\\\10] 1\n\"SteamPath\"=\"wrong\"\n\n\
             [Software\\\\Valve\\\\Steam] 1794240603\n#time=1dc\n\"SteamExe\"=\"c:/games/steam/steam.exe\"\n\
             \"SteamPath\"=\"c:\\\\games\\\\steam\"\n",
        );
        assert_eq!(registered_steam_path(dir.path()).as_deref(), Some(r"c:\games\steam"));
    }

    #[test]
    fn finds_library_artwork() {
        let dir = TempDir::new("steam-art");
        let prefix = dir.path();
        let menu = prefix.join("drive_c/users/me/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Steam");
        // Steam in a custom folder, recorded in the registry.
        write(&prefix.join("user.reg"), "[Software\\\\Valve\\\\Steam] 1\n\"SteamPath\"=\"c:/games/steam\"\n");
        let cache = prefix.join("drive_c/Games/Steam/appcache/librarycache");
        for name in ["library_hero.jpg", "logo.png", "library_600x900.jpg", "header.jpg"] {
            write(&cache.join("287450").join(name), b"jpg");
        }
        write(&cache.join("10_header.jpg"), b"jpg"); // the older flat layout
        let game = menu.join("Rise of Nations.url");
        write(&game, url("steam://rungameid/287450"));
        let art = artwork(&game, prefix);
        assert_eq!(art.steam_app_id, Some(287450));
        let hero = art.hero.expect("hero");
        assert!(hero.ends_with("287450/library_hero.jpg") && hero.is_file());
        assert!(art.logo.is_some() && art.cover.is_some() && art.header.is_some());

        let old = menu.join("Counter-Strike.url");
        write(&old, url("steam://rungameid/10"));
        let art = artwork(&old, prefix);
        assert_eq!((art.steam_app_id, art.hero, art.logo), (Some(10), None, None));
        assert!(art.header.is_some_and(|p| p.ends_with("10_header.jpg")));

        // Not a Steam game: a web link, a .lnk, a missing file.
        let web = menu.join("Support.url");
        write(&web, url("http://support.steampowered.com/"));
        assert_eq!(artwork(&web, prefix), Artwork::default());
        let link = menu.join("Steam.lnk");
        write(&link, crate::icons::fixtures::lnk(Some(r"C:\Games\Steam\steam.exe"), None, 0));
        assert_eq!(artwork(&link, prefix), Artwork::default());
        assert_eq!(artwork(&menu.join("gone.url"), prefix), Artwork::default());
    }

    #[test]
    fn finds_steam_in_its_default_folder() {
        let dir = TempDir::new("steam-default");
        let prefix = dir.path();
        write(&prefix.join("drive_c/Program Files (x86)/Steam/appcache/librarycache/7/logo.png"), b"png");
        let game = prefix.join("game.url");
        write(&game, url("steam://rungameid/7"));
        assert!(artwork(&game, prefix).logo.is_some());
    }
}
