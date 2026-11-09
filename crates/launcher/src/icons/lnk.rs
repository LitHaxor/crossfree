//! Windows shortcuts (`.lnk`, [MS-SHLLINK]): what a shortcut runs, and
//! where its icon is.
//!
//! [MS-SHLLINK]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-shllink

use super::{IconSource, u16_at, u32_at, utf16};

const HEADER_SIZE: usize = 0x4c;
const LINK_CLSID: [u8; 16] = [0x01, 0x14, 0x02, 0x00, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

const HAS_ID_LIST: u32 = 1 << 0;
const HAS_LINK_INFO: u32 = 1 << 1;
const HAS_RELATIVE_PATH: u32 = 1 << 3;
const HAS_ICON_LOCATION: u32 = 1 << 6;
const IS_UNICODE: u32 = 1 << 7;
const HAS_EXP_STRING: u32 = 1 << 9;
const HAS_EXP_ICON: u32 = 1 << 14;
/// The StringData fields in file order: name, relative path, working
/// directory, arguments, icon location.
const STRINGS: [u32; 5] = [1 << 2, HAS_RELATIVE_PATH, 1 << 4, 1 << 5, HAS_ICON_LOCATION];

const ENVIRONMENT_BLOCK: u32 = 0xa000_0001;
const ICON_ENVIRONMENT_BLOCK: u32 = 0xa000_0007;

/// The parts of a shortcut that say what it runs and what it looks like.
struct Link {
    flags: u32,
    index: i32,
    /// LinkInfo's local path.
    target: Option<String>,
    /// The EnvironmentVariableDataBlock's path (`%VAR%` not expanded).
    environment_target: Option<String>,
    relative_path: Option<String>,
    icon_location: Option<String>,
    environment_icon: Option<String>,
}

impl Link {
    /// What the shortcut runs: LinkInfo's path, else the environment
    /// block's (when the link says it has one), else the relative path.
    fn program(&self) -> Option<String> {
        self.target
            .clone()
            .or_else(|| self.environment_target.clone().filter(|_| self.flags & HAS_EXP_STRING != 0))
            .or_else(|| self.relative_path.clone())
            .filter(|p| !p.is_empty())
    }
}

/// Where the icon of the shortcut `data` is, as the Windows shell (and
/// Wine's `winemenubuilder`) finds it: the link's icon location if it has
/// one, otherwise its target. The path is a Windows path, possibly relative
/// to the shortcut's folder.
pub(super) fn icon_source(data: &[u8]) -> Option<IconSource> {
    let link = parse(data)?;
    let icon = link
        .environment_icon
        .clone()
        .filter(|_| link.flags & HAS_EXP_ICON != 0)
        .or_else(|| link.icon_location.clone())
        .filter(|p| !p.is_empty());
    let path = icon.or_else(|| link.program())?;
    Some(IconSource { path, index: link.index })
}

/// The program the shortcut `data` runs, as a Windows path (possibly
/// relative to the shortcut's folder, or holding `%VARIABLES%`).
pub(super) fn target(data: &[u8]) -> Option<String> {
    parse(data)?.program()
}

fn parse(data: &[u8]) -> Option<Link> {
    if u32_at(data, 0)? != HEADER_SIZE as u32 || data.get(4..20)? != LINK_CLSID {
        return None;
    }
    let flags = u32_at(data, 20)?;
    let index = i32::from_le_bytes(data.get(56..60)?.try_into().ok()?);
    let mut pos = HEADER_SIZE;
    if flags & HAS_ID_LIST != 0 {
        pos = pos.checked_add(2 + usize::from(u16_at(data, pos)?))?;
    }
    let mut target = None;
    if flags & HAS_LINK_INFO != 0 {
        let size = usize::try_from(u32_at(data, pos)?).ok()?;
        target = local_path(data.get(pos..pos.checked_add(size)?)?);
        pos += size;
    }
    let mut strings: [Option<String>; 5] = Default::default();
    for (slot, flag) in strings.iter_mut().zip(STRINGS) {
        if flags & flag != 0 {
            let (text, next) = string(data, pos, flags & IS_UNICODE != 0)?;
            *slot = Some(text);
            pos = next;
        }
    }
    let [_, relative_path, _, _, icon_location] = strings;

    // ExtraData: blocks of (size, signature, ...), ending with a size below 4.
    let (mut environment_target, mut environment_icon) = (None, None);
    while let Some(size) = u32_at(data, pos).and_then(|s| usize::try_from(s).ok()) {
        let Some(block) = data.get(pos..pos.saturating_add(size)).filter(|_| size >= 8) else { break };
        match u32_at(block, 4) {
            Some(ENVIRONMENT_BLOCK) => environment_target = environment_path(block),
            Some(ICON_ENVIRONMENT_BLOCK) => environment_icon = environment_path(block),
            _ => {}
        }
        pos += size;
    }
    Some(Link { flags, index, target, environment_target, relative_path, icon_location, environment_icon })
}

/// A StringData string at `pos`: a character count, then UTF-16 or ANSI
/// text. Returns the text (up to any NUL) and the position after it.
fn string(data: &[u8], pos: usize, unicode: bool) -> Option<(String, usize)> {
    let count = usize::from(u16_at(data, pos)?);
    let start = pos + 2;
    let len = if unicode { count * 2 } else { count };
    let bytes = data.get(start..start.checked_add(len)?)?;
    let text = if unicode { utf16(bytes) } else { ansi(bytes) };
    Some((text, start + len))
}

/// ANSI text up to the first NUL, read as Latin-1 (exact for ASCII paths).
fn ansi(bytes: &[u8]) -> String {
    bytes.iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect()
}

/// The local path in a LinkInfo structure: base path plus common path
/// suffix, from the Unicode fields when present.
fn local_path(info: &[u8]) -> Option<String> {
    let field = |at: usize| u32_at(info, at).and_then(|o| usize::try_from(o).ok());
    let has_local_path = u32_at(info, 8)? & 1 != 0;
    if !has_local_path {
        return None;
    }
    if field(4)? >= 0x24 {
        let base = field(28)?;
        if base != 0 {
            let suffix = field(32).filter(|&o| o != 0).and_then(|o| info.get(o..)).map(utf16);
            return Some(utf16(info.get(base..)?) + &suffix.unwrap_or_default());
        }
    }
    let suffix = field(24).filter(|&o| o != 0).and_then(|o| info.get(o..)).map(ansi);
    Some(ansi(info.get(field(16)?..)?) + &suffix.unwrap_or_default())
}

/// The path in an EnvironmentVariableDataBlock or IconEnvironmentDataBlock:
/// 260 bytes of ANSI text, then 520 bytes of UTF-16.
fn environment_path(block: &[u8]) -> Option<String> {
    let unicode = block.get(268..788).map(utf16).filter(|s| !s.is_empty());
    unicode.or_else(|| block.get(8..268).map(ansi)).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::fixtures::{self, Rng};

    const STEAM_LNK: &[u8] = include_bytes!("testdata/steam.lnk");

    fn source(path: &str, index: i32) -> Option<IconSource> {
        Some(IconSource { path: path.into(), index })
    }

    #[test]
    fn reads_a_shortcut_written_by_wine() {
        // Steam's installer made this one through Wine's IShellLink: an ID
        // list, LinkInfo with the target, and no icon location.
        assert_eq!(icon_source(STEAM_LNK), source(r"C:\Program Files (x86)\Steam\steam.exe", 0));
        assert_eq!(target(STEAM_LNK).as_deref(), Some(r"C:\Program Files (x86)\Steam\steam.exe"));
    }

    /// What a shortcut runs ignores its icon.
    #[test]
    fn the_target_is_the_program() {
        let link = fixtures::lnk(Some(r"C:\Games\game.exe"), Some(r"C:\Games\icons.dll"), -101);
        assert_eq!(target(&link).as_deref(), Some(r"C:\Games\game.exe"));
        assert_eq!(target(&fixtures::lnk(None, Some(r"C:\Games\icons.dll"), 0)), None);
        assert_eq!(target(b"[InternetShortcut]\r\nURL=steam://rungameid/1\r\n"), None);
    }

    #[test]
    fn prefers_the_icon_location() {
        let link = fixtures::lnk(Some(r"C:\Games\game.exe"), Some(r"C:\Games\icons.dll"), -101);
        assert_eq!(icon_source(&link), source(r"C:\Games\icons.dll", -101));
        let link = fixtures::lnk(Some(r"C:\Games\game.exe"), None, 2);
        assert_eq!(icon_source(&link), source(r"C:\Games\game.exe", 2));
        let link = fixtures::lnk(Some(r"C:\Games\game.exe"), Some(""), 0);
        assert_eq!(icon_source(&link), source(r"C:\Games\game.exe", 0));
        assert_eq!(icon_source(&fixtures::lnk(None, None, 0)), None);
    }

    #[test]
    fn uses_the_icon_environment_block() {
        let mut link = fixtures::lnk(None, Some(r"C:\old.ico"), 0);
        let flags = u32_at(&link, 20).unwrap() | HAS_EXP_ICON;
        link[20..24].copy_from_slice(&flags.to_le_bytes());
        link.truncate(link.len() - 4); // the terminal block
        let mut block = vec![0; 0x314];
        block[..4].copy_from_slice(&0x314u32.to_le_bytes());
        block[4..8].copy_from_slice(&ICON_ENVIRONMENT_BLOCK.to_le_bytes());
        block[8..8 + 19].copy_from_slice(b"%SystemRoot%\\a.ico\0");
        let wide: Vec<u8> = r"%ProgramFiles%\b.ico".encode_utf16().flat_map(u16::to_le_bytes).collect();
        block[268..268 + wide.len()].copy_from_slice(&wide);
        link.extend_from_slice(&block);
        link.extend_from_slice(&[0; 4]);
        assert_eq!(icon_source(&link), source(r"%ProgramFiles%\b.ico", 0));
        // Without the flag the block is ignored.
        link[20..24].copy_from_slice(&(flags & !HAS_EXP_ICON).to_le_bytes());
        assert_eq!(icon_source(&link), source(r"C:\old.ico", 0));
    }

    #[test]
    fn reads_unicode_link_info() {
        let wide: Vec<u8> = "C:\\Spiele\\Größe.exe\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut info = vec![0; 0x24];
        info[4..8].copy_from_slice(&0x24u32.to_le_bytes());
        info[8..12].copy_from_slice(&1u32.to_le_bytes());
        info[28..32].copy_from_slice(&0x24u32.to_le_bytes());
        info.extend_from_slice(&wide);
        let size = info.len() as u32;
        info[..4].copy_from_slice(&size.to_le_bytes());
        assert_eq!(local_path(&info).as_deref(), Some("C:\\Spiele\\Größe.exe"));
        info[8] = 0; // no local path (a network link)
        assert_eq!(local_path(&info), None);
    }

    #[test]
    fn rejects_other_files() {
        assert_eq!(icon_source(b""), None);
        assert_eq!(icon_source(b"[InternetShortcut]\r\nURL=steam://rungameid/1\r\n"), None);
        let mut link = STEAM_LNK.to_vec();
        link[4] ^= 1; // wrong CLSID
        assert_eq!(icon_source(&link), None);
    }

    #[test]
    fn malformed_links_never_panic() {
        for len in 0..STEAM_LNK.len() {
            let _ = icon_source(&STEAM_LNK[..len]);
        }
        let mut rng = Rng(0x9e37_79b9);
        for _ in 0..5000 {
            let mut link = STEAM_LNK.to_vec();
            for _ in 0..1 + rng.below(6) {
                let at = rng.below(link.len());
                link[at] = rng.next() as u8;
            }
            let _ = icon_source(&link);
            let _ = target(&link);
        }
    }
}
