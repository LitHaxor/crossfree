//! App icons for Start Menu shortcuts.
//!
//! A shortcut names its icon the way the Windows shell expects; Wine's
//! `winemenubuilder` (which CrossOver runs to make its app icons) reads it
//! the same way:
//! - `.lnk`: the link's icon location, or else its target program;
//! - `.url`: the `IconFile` and `IconIndex` keys.
//!
//! The icon is a group in an `.exe`/`.dll`, or an `.ico` file. It is written
//! to a cache directory as an `.ico` file with every size, which macOS reads
//! natively and scales from the best size. A cache file's name comes from the
//! source file's path, size and modification time, so its content never
//! changes, and an updated program gets a new file.

mod lnk;
mod pe;
mod steam;

#[cfg(test)]
pub(crate) mod fixtures;

pub use steam::Artwork;
pub(crate) use steam::{artwork, install_dir};

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Largest shortcut file read.
const MAX_SHORTCUT_BYTES: u64 = 1 << 20;

/// Where a shortcut's icon is: a Windows path, and an `ExtractIcon` index.
#[derive(Debug, PartialEq, Eq)]
struct IconSource {
    path: String,
    index: i32,
}

/// `~/Library/Caches/crossfree/icons`: where extracted icons are kept.
pub fn icon_cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Caches/crossfree/icons"))
}

/// The icon of the shortcut (`.lnk` or `.url`) at `shortcut` in `prefix`, as
/// an `.ico` file in `cache`; `None` if the shortcut has no usable icon.
pub(crate) fn icon_for_shortcut(shortcut: &Path, prefix: &Path, cache: &Path) -> Option<PathBuf> {
    let data = read_small(shortcut)?;
    let extension = shortcut.extension()?.to_str()?.to_ascii_lowercase();
    let source = match extension.as_str() {
        "lnk" => lnk::icon_source(&data)?,
        "url" => url_icon_source(&text(&data))?,
        _ => return None,
    };
    let file = resolve(&source.path, prefix, shortcut.parent()?)?;
    cached(&file, source.index, cache)
}

fn read_small(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SHORTCUT_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

/// An INI-style file's text: UTF-16 if it starts with that byte-order mark,
/// otherwise UTF-8 (lossy).
fn text(data: &[u8]) -> String {
    if let Some(wide) = data.strip_prefix(&[0xff, 0xfe]) {
        return utf16(wide);
    }
    let text = String::from_utf8_lossy(data);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
}

/// The trimmed `key=value` lines of an INI file's `[section]` (section names
/// match without regard to case).
fn ini_section<'a>(text: &'a str, section: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
    let mut in_section = false;
    text.lines().map(str::trim).filter_map(move |line| {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = name.eq_ignore_ascii_case(section);
            return None;
        }
        let (key, value) = line.split_once('=').filter(|_| in_section)?;
        Some((key.trim(), value.trim()))
    })
}

/// The Windows program the `.lnk` shortcut at `shortcut` runs, as an
/// absolute Windows path (`C:\…\steam.exe`, `%VARIABLES%` expanded): the
/// name Wine gives the program's process. `None` for other shortcuts and
/// for targets that are not absolute.
pub(crate) fn shortcut_program(shortcut: &Path) -> Option<String> {
    if !shortcut.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
        return None;
    }
    let target = lnk::target(&read_small(shortcut)?)?;
    let target = if target.contains('%') { expand_variables(&target)? } else { target };
    let bytes = target.as_bytes();
    let absolute = bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    absolute.then_some(target)
}

/// Whether `shortcut` is an Internet shortcut (`.url`) to a web page or an
/// address (`http:`, `https:`, `ftp:`, `mailto:`), such as the "Steam
/// Support Center" link Steam puts in the Start Menu: a link, not a
/// program. Steam's games are `steam:` links, and are programs.
pub(crate) fn is_web_link(shortcut: &Path) -> bool {
    let is_url = shortcut.extension().is_some_and(|e| e.eq_ignore_ascii_case("url"));
    let Some(data) = read_small(shortcut).filter(|_| is_url) else { return false };
    let text = text(&data);
    let Some((_, url)) = ini_section(&text, "InternetShortcut").find(|(k, _)| k.eq_ignore_ascii_case("URL")) else {
        return false;
    };
    let scheme = url.split_once(':').map_or("", |(scheme, _)| scheme);
    ["http", "https", "ftp", "mailto"].iter().any(|web| scheme.eq_ignore_ascii_case(web))
}

/// The icon of an Internet shortcut: `IconFile` and `IconIndex` in its
/// `[InternetShortcut]` section (key case varies; Steam writes `ICONFILE`).
fn url_icon_source(text: &str) -> Option<IconSource> {
    let (mut path, mut index) = (None, 0);
    for (key, value) in ini_section(text, "InternetShortcut") {
        if key.eq_ignore_ascii_case("IconFile") {
            path = Some(value.to_owned()).filter(|v| !v.is_empty());
        } else if key.eq_ignore_ascii_case("IconIndex") {
            index = value.parse().unwrap_or(0);
        }
    }
    Some(IconSource { path: path?, index })
}

/// The file a Windows path names in `prefix` (see [`locate`]).
fn resolve(windows: &str, prefix: &Path, base: &Path) -> Option<PathBuf> {
    locate(windows, prefix, base).filter(|path| path.is_file())
}

/// The existing file or folder a Windows path names in `prefix`: `X:\…`
/// through the prefix's `dosdevices`, a relative path from `base`. Names
/// match without regard to case, as on Windows, and `..` stops at the drive
/// root (or, for a relative path, at the prefix).
fn locate(windows: &str, prefix: &Path, base: &Path) -> Option<PathBuf> {
    let path = expand_variables(windows.trim().trim_matches('"'))?;
    let (root, floor, rest) = match path.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => {
            let root = drive(prefix, char::from(*letter))?;
            (root.clone(), root, &path[2..])
        }
        // UNC paths and paths relative to the current drive's root.
        [b'\\' | b'/', ..] | [] => return None,
        _ => (base.to_path_buf(), prefix.to_path_buf(), path.as_str()),
    };
    let mut current = root;
    for part in rest.split(['\\', '/']) {
        match part {
            "" | "." => {}
            ".." => {
                if current != floor && current.starts_with(&floor) {
                    current.pop();
                }
            }
            name => current = child(&current, name)?,
        }
    }
    current.exists().then_some(current)
}

/// The directory for drive `letter`: `dosdevices/x:`, or `drive_c` for C:.
fn drive(prefix: &Path, letter: char) -> Option<PathBuf> {
    let letter = letter.to_ascii_lowercase();
    let device = prefix.join("dosdevices").join(format!("{letter}:"));
    if device.is_dir() {
        return Some(device);
    }
    let drive_c = prefix.join("drive_c");
    (letter == 'c' && drive_c.is_dir()).then_some(drive_c)
}

/// `dir/name`, or the entry of `dir` whose name matches `name` ignoring case.
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.symlink_metadata().is_ok() {
        return Some(exact);
    }
    let wanted = name.to_lowercase();
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_str().is_some_and(|n| n.to_lowercase() == wanted))
        .map(|e| e.path())
}

/// Expands `%NAME%` for the system folders shortcuts refer to. `None` for
/// other variables, which depend on the user or the process.
fn expand_variables(path: &str) -> Option<String> {
    const VARIABLES: [(&str, &str); 12] = [
        ("SystemDrive", "C:"),
        ("SystemRoot", r"C:\windows"),
        ("windir", r"C:\windows"),
        ("ProgramFiles", r"C:\Program Files"),
        ("ProgramW6432", r"C:\Program Files"),
        ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        ("CommonProgramFiles", r"C:\Program Files\Common Files"),
        ("CommonProgramW6432", r"C:\Program Files\Common Files"),
        ("CommonProgramFiles(x86)", r"C:\Program Files (x86)\Common Files"),
        ("ProgramData", r"C:\ProgramData"),
        ("ALLUSERSPROFILE", r"C:\ProgramData"),
        ("PUBLIC", r"C:\users\Public"),
    ];
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let (_, value) = VARIABLES.iter().find(|(name, _)| name.eq_ignore_ascii_case(&after[..end]))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The icon `index` of `file` as an `.ico` file in `cache`, extracted the
/// first time it is asked for.
fn cached(file: &Path, index: i32, cache: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(file).ok()?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    let key = fnv1a(&[
        file.as_os_str().as_bytes(),
        &[0],
        &index.to_le_bytes(),
        &meta.len().to_le_bytes(),
        &modified.to_le_bytes(),
    ]);
    let out = cache.join(format!("{key:016x}.ico"));
    if out.is_file() {
        return Some(out);
    }
    let icon = pe::icon(file, index)?;
    std::fs::create_dir_all(cache).ok()?;
    let temp = cache.join(format!(".{key:016x}.{}.tmp", std::process::id()));
    if std::fs::write(&temp, icon).and_then(|()| std::fs::rename(&temp, &out)).is_err() {
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    Some(out)
}

/// 64-bit FNV-1a: a hash that stays the same across Rust releases, for
/// cache file names.
fn fnv1a(parts: &[&[u8]]) -> u64 {
    parts
        .iter()
        .flat_map(|p| p.iter())
        .fold(0xcbf2_9ce4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// UTF-16LE text up to the first NUL.
fn utf16(bytes: &[u8]) -> String {
    let units = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0);
    char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn write(path: &Path, data: impl AsRef<[u8]>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    /// A prefix laid out like Wine's: `dosdevices/c:` links to `drive_c`.
    fn prefix(dir: &TempDir) -> PathBuf {
        let prefix = dir.path().join("Steam");
        std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        prefix
    }

    fn source(path: &str, index: i32) -> Option<IconSource> {
        Some(IconSource { path: path.into(), index })
    }

    /// Whether `found` is the file `expected` (whatever the path's case).
    fn is_file(found: Option<PathBuf>, expected: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        let id = |p: &Path| std::fs::metadata(p).map(|m| (m.dev(), m.ino())).ok();
        found.is_some_and(|f| id(&f).is_some() && id(&f) == id(expected))
    }

    #[test]
    fn reads_internet_shortcuts() {
        // As Steam writes them for games.
        let steam = "[InternetShortcut]\r\nURL=steam://rungameid/287450\r\nICONFILE=C:\\Program Files (x86)\\Steam\\steam\\games\\0f76.ico\r\nICONINDEX=0\r\n";
        assert_eq!(url_icon_source(steam), source(r"C:\Program Files (x86)\Steam\steam\games\0f76.ico", 0));
        let mixed = "[{000214A0-0000-0000-C000-000000000046}]\nIconFile=C:\\wrong.ico\n[internetshortcut]\n IconFile = C:\\x.dll \nIconIndex=-3\n";
        assert_eq!(url_icon_source(mixed), source(r"C:\x.dll", -3));
        assert_eq!(url_icon_source("[InternetShortcut]\nURL=http://support.steampowered.com/\n"), None);
        assert_eq!(url_icon_source("[InternetShortcut]\nIconFile=\nIconIndex=2\n"), None);
        assert_eq!(url_icon_source("IconFile=C:\\outside-any-section.ico\n"), None);
        assert_eq!(url_icon_source("[InternetShortcut]\nIconFile=C:\\a.ico\nIconIndex=junk\n"), source(r"C:\a.ico", 0));
    }

    #[test]
    fn decodes_shortcut_text() {
        assert_eq!(text(b"\xef\xbb\xbf[InternetShortcut]"), "[InternetShortcut]");
        let wide: Vec<u8> =
            [0xff, 0xfe].into_iter().chain("[A]\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(text(&wide), "[A]\r\n");
    }

    #[test]
    fn expands_system_folder_variables() {
        assert_eq!(
            expand_variables(r"%SystemRoot%\system32\shell32.dll").as_deref(),
            Some(r"C:\windows\system32\shell32.dll")
        );
        assert_eq!(expand_variables(r"%programfiles(x86)%\Steam").as_deref(), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(expand_variables("plain").as_deref(), Some("plain"));
        assert_eq!(expand_variables(r"%APPDATA%\x.ico"), None);
        assert_eq!(expand_variables("50% off"), None);
    }

    #[test]
    fn resolves_windows_paths_in_a_prefix() {
        let dir = TempDir::new("resolve");
        let prefix = prefix(&dir);
        let exe = prefix.join("drive_c/Program Files (x86)/Steam/steam.exe");
        write(&exe, b"MZ");
        let menu = prefix.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs/Steam");
        std::fs::create_dir_all(&menu).unwrap();
        let c = prefix.join("dosdevices/c:");
        let via_c = c.join("Program Files (x86)/Steam/steam.exe");

        assert_eq!(resolve(r"C:\Program Files (x86)\Steam\steam.exe", &prefix, &menu), Some(via_c.clone()));
        assert!(is_file(resolve(r#""c:/program files (x86)/STEAM/Steam.EXE""#, &prefix, &menu), &exe));
        assert_eq!(resolve(r"%ProgramFiles(x86)%\Steam\.\steam.exe", &prefix, &menu), Some(via_c.clone()));
        // `..` cannot climb above the drive.
        assert_eq!(resolve(r"C:\..\..\Program Files (x86)\Steam\steam.exe", &prefix, &menu), Some(via_c));
        // Relative to the shortcut's folder.
        let relative = r"..\..\..\..\..\..\Program Files (x86)\Steam\steam.exe";
        assert!(is_file(resolve(relative, &prefix, &menu), &exe));
        // Without dosdevices, C: is drive_c.
        std::fs::remove_file(&c).unwrap();
        assert_eq!(resolve(r"C:\Program Files (x86)\Steam\steam.exe", &prefix, &menu), Some(exe));

        for missing in [
            r"C:\Program Files (x86)\Steam",
            r"C:\nope.exe",
            r"D:\steam.exe",
            r"\\server\share\a.exe",
            r"\a.exe",
            "",
            r"%APPDATA%\a.exe",
        ] {
            assert_eq!(resolve(missing, &prefix, &menu), None, "{missing}");
        }
    }

    #[test]
    fn extracts_and_caches_icons() {
        let dir = TempDir::new("icons");
        let prefix = prefix(&dir);
        let cache = dir.path().join("cache");
        let menu = prefix.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs");
        let exe = prefix.join("drive_c/Games/Game/game.exe");
        let images = [fixtures::image(1), fixtures::image(2)];
        write(&exe, fixtures::pe(false, &[(1, vec![(1, images[0].clone()), (2, images[1].clone())])]));
        let link = menu.join("Game.lnk");
        write(&link, fixtures::lnk(Some(r"C:\GAMES\game\GAME.exe"), None, 0));

        let icon = icon_for_shortcut(&link, &prefix, &cache).expect("icon from the target");
        assert!(icon.starts_with(&cache) && icon.extension().is_some_and(|e| e == "ico"));
        assert_eq!(std::fs::read(&icon).unwrap(), fixtures::ico(&images));
        assert_eq!(icon_for_shortcut(&link, &prefix, &cache), Some(icon.clone()), "cached");

        // A changed program gets a new icon file.
        write(&exe, fixtures::pe(false, &[(1, vec![(1, fixtures::image(7))])]));
        let changed = icon_for_shortcut(&link, &prefix, &cache).unwrap();
        assert_ne!(changed, icon);
        assert_eq!(std::fs::read(&changed).unwrap(), fixtures::ico(&[fixtures::image(7)]));

        // An Internet shortcut with an .ico file, as Steam makes for games.
        let ico = fixtures::ico(&[fixtures::image(3)]);
        write(&prefix.join("drive_c/Steam/steam/games/abc.ico"), &ico);
        let url = menu.join("Game 2.url");
        write(
            &url,
            "[InternetShortcut]\r\nURL=steam://rungameid/1\r\nICONFILE=C:\\Steam\\steam\\games\\abc.ico\r\nICONINDEX=0\r\n",
        );
        assert_eq!(std::fs::read(icon_for_shortcut(&url, &prefix, &cache).unwrap()).unwrap(), ico);

        // No icon: missing file, no IconFile, not an icon, unknown extension.
        write(&url, "[InternetShortcut]\r\nURL=steam://rungameid/1\r\nICONFILE=C:\\Steam\\steam\\games\\gone.ico\r\n");
        assert_eq!(icon_for_shortcut(&url, &prefix, &cache), None);
        write(&url, "[InternetShortcut]\r\nURL=http://support.steampowered.com/\r\n");
        assert_eq!(icon_for_shortcut(&url, &prefix, &cache), None);
        write(&link, fixtures::lnk(Some(r"C:\Steam\steam\games\abc.ico"), Some(r"C:\Games\readme.txt"), 0));
        write(&prefix.join("drive_c/Games/readme.txt"), "hello");
        assert_eq!(icon_for_shortcut(&link, &prefix, &cache), None);
        let other = menu.join("Game.pif");
        write(&other, fixtures::lnk(Some(r"C:\Games\Game\game.exe"), None, 0));
        assert_eq!(icon_for_shortcut(&other, &prefix, &cache), None);
        assert_eq!(icon_for_shortcut(&menu.join("missing.lnk"), &prefix, &cache), None);
    }

    #[test]
    fn cache_names_are_stable() {
        // Cache file names must not change between builds of the launcher.
        assert_eq!(fnv1a(&[]), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(&[b"a"]), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(&[b"fo", b"obar"]), fnv1a(&[b"foobar"]));
        assert_eq!(fnv1a(&[b"foobar"]), 0x8594_4171_f739_67e8);
    }
}
