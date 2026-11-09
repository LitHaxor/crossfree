//! Icons inside Windows executables, and `.ico` files.
//!
//! A program's icon is an `RT_GROUP_ICON` resource: a directory of images,
//! each stored as its own `RT_ICON` resource. [`icon`] rebuilds one group as
//! a standalone `.ico` file, which macOS (ImageIO) reads natively.
//!
//! The input is untrusted: every offset is checked, sizes are capped, and
//! only the headers and resources needed are read, so a large game
//! executable costs a few small reads.

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

use super::{u16_at, u32_at};

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
/// Images kept from one icon group.
const MAX_IMAGES: usize = 64;
/// Largest single resource read (a 256×256 32-bit bitmap is about 260 KB).
const MAX_RESOURCE_BYTES: u32 = 4 << 20;
/// Largest `.ico` file produced or accepted.
const MAX_ICO_BYTES: usize = 16 << 20;
/// Sections in a PE file (the Windows loader's limit).
const MAX_SECTIONS: usize = 96;
/// Entries in one resource directory.
const MAX_DIRECTORY_ENTRIES: usize = 4096;

/// The icon `index` of the executable or `.ico` file at `path`, as the bytes
/// of an `.ico` file.
///
/// `index` follows `ExtractIcon`: `n >= 0` is the n-th icon group in
/// resource order, `-id` is the group with resource ID `id`. An `.ico` file
/// holds one icon, so it ignores `index`.
pub(super) fn icon(path: &Path, index: i32) -> Option<Vec<u8>> {
    icon_in(&FileBytes::open(path)?, index)
}

fn icon_in<R: ReadAt + ?Sized>(file: &R, index: i32) -> Option<Vec<u8>> {
    let magic = file.read_at(0, 4)?;
    if magic.starts_with(b"MZ") {
        Pe::parse(file)?.group_icon(index)
    } else if magic == [0, 0, 1, 0] {
        let size = usize::try_from(file.size()).ok().filter(|&n| n <= MAX_ICO_BYTES)?;
        let data = file.read_at(0, size)?;
        is_ico(&data).then_some(data)
    } else {
        None
    }
}

/// Random-access reads: `None` when the range is outside the data.
trait ReadAt {
    fn size(&self) -> u64;
    fn read_at(&self, offset: u64, len: usize) -> Option<Vec<u8>>;
}

impl ReadAt for [u8] {
    fn size(&self) -> u64 {
        self.len() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> Option<Vec<u8>> {
        let start = usize::try_from(offset).ok()?;
        Some(self.get(start..start.checked_add(len)?)?.to_vec())
    }
}

struct FileBytes {
    file: File,
    size: u64,
}

impl FileBytes {
    fn open(path: &Path) -> Option<Self> {
        let file = File::open(path).ok()?;
        let meta = file.metadata().ok()?;
        meta.is_file().then(|| Self { file, size: meta.len() })
    }
}

impl ReadAt for FileBytes {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, len: usize) -> Option<Vec<u8>> {
        if offset.checked_add(u64::try_from(len).ok()?)? > self.size {
            return None;
        }
        let mut buf = vec![0; len];
        self.file.read_exact_at(&mut buf, offset).ok()?;
        Some(buf)
    }
}

struct Section {
    address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

/// A resource directory entry's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Id(u32),
    Name,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    key: Key,
    /// Offset from the start of the resource data.
    target: u32,
    /// Whether `target` is a subdirectory (otherwise a data entry).
    is_directory: bool,
}

/// The parts of a PE file needed to read its resources.
struct Pe<'a, R: ?Sized> {
    file: &'a R,
    sections: Vec<Section>,
    /// RVA of the resource directory.
    resources: u32,
}

impl<'a, R: ReadAt + ?Sized> Pe<'a, R> {
    fn parse(file: &'a R) -> Option<Self> {
        let dos = file.read_at(0, 64)?;
        let header = u64::from(u32_at(&dos, 0x3c)?);
        let coff = file.read_at(header, 24)?;
        if !coff.starts_with(b"PE\0\0") {
            return None;
        }
        let section_count = usize::from(u16_at(&coff, 6)?);
        let optional_size = u16_at(&coff, 20)?;
        let optional = file.read_at(header + 24, usize::from(optional_size))?;
        // Offsets of NumberOfRvaAndSizes and of the data directories.
        let (count_at, directories_at) = match u16_at(&optional, 0)? {
            0x10b => (92, 96),   // PE32
            0x20b => (108, 112), // PE32+
            _ => return None,
        };
        if u32_at(&optional, count_at)? <= 2 || section_count > MAX_SECTIONS {
            return None;
        }
        // Data directory 2 is the resource table.
        let resources = u32_at(&optional, directories_at + 16)?;
        if resources == 0 {
            return None;
        }
        let table = file.read_at(header + 24 + u64::from(optional_size), section_count * 40)?;
        let sections = table
            .chunks_exact(40)
            .map(|s| {
                Some(Section {
                    virtual_size: u32_at(s, 8)?,
                    address: u32_at(s, 12)?,
                    raw_size: u32_at(s, 16)?,
                    raw_offset: u32_at(s, 20)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { file, sections, resources })
    }

    /// `len` bytes at `rva`, which must lie in one section's file data.
    fn read_rva(&self, rva: u32, len: usize) -> Option<Vec<u8>> {
        let section =
            self.sections.iter().find(|s| rva >= s.address && rva - s.address < s.virtual_size.max(s.raw_size))?;
        let delta = rva - section.address;
        if delta.checked_add(u32::try_from(len).ok()?)? > section.raw_size {
            return None;
        }
        self.file.read_at(u64::from(section.raw_offset) + u64::from(delta), len)
    }

    /// The entries of the resource directory at `offset`, in file order
    /// (named entries first, then IDs in ascending order).
    fn directory(&self, offset: u32) -> Option<Vec<Entry>> {
        let rva = self.resources.checked_add(offset)?;
        let header = self.read_rva(rva, 16)?;
        let count = usize::from(u16_at(&header, 12)?) + usize::from(u16_at(&header, 14)?);
        if count > MAX_DIRECTORY_ENTRIES {
            return None;
        }
        self.read_rva(rva.checked_add(16)?, count * 8)?
            .chunks_exact(8)
            .map(|e| {
                let (name, target) = (u32_at(e, 0)?, u32_at(e, 4)?);
                Some(Entry {
                    key: if name & 0x8000_0000 == 0 { Key::Id(name) } else { Key::Name },
                    target: target & 0x7fff_ffff,
                    is_directory: target & 0x8000_0000 != 0,
                })
            })
            .collect()
    }

    /// The subdirectory for resource type `id`.
    fn type_directory(root: &[Entry], id: u32) -> Option<u32> {
        root.iter().find(|e| e.key == Key::Id(id) && e.is_directory).map(|e| e.target)
    }

    /// The data of a resource: the first language under its name directory.
    fn resource(&self, name: &Entry) -> Option<Vec<u8>> {
        if !name.is_directory {
            return None;
        }
        let language = self.directory(name.target)?.into_iter().find(|e| !e.is_directory)?;
        let entry = self.read_rva(self.resources.checked_add(language.target)?, 16)?;
        let size = u32_at(&entry, 4)?;
        if size > MAX_RESOURCE_BYTES {
            return None;
        }
        self.read_rva(u32_at(&entry, 0)?, usize::try_from(size).ok()?)
    }

    fn group_icon(&self, index: i32) -> Option<Vec<u8>> {
        let root = self.directory(0)?;
        let groups = self.directory(Self::type_directory(&root, RT_GROUP_ICON)?)?;
        let group = match usize::try_from(index) {
            Ok(n) => groups.get(n)?,
            Err(_) => groups.iter().find(|e| e.key == Key::Id(index.unsigned_abs()))?,
        };
        // GRPICONDIR: reserved, type (1 = icon), count, then 14-byte entries.
        let directory = self.resource(group)?;
        if u16_at(&directory, 0)? != 0 || u16_at(&directory, 2)? != 1 {
            return None;
        }
        let count = usize::from(u16_at(&directory, 4)?).min(MAX_IMAGES);
        let icons = self.directory(Self::type_directory(&root, RT_ICON)?)?;
        let mut images = Vec::new();
        let mut total = 0;
        for entry in directory.get(6..)?.chunks_exact(14).take(count) {
            let id = u32::from(u16_at(entry, 12)?);
            let Some(icon) = icons.iter().find(|e| e.key == Key::Id(id)) else { continue };
            let Some(data) = self.resource(icon).filter(|d| !d.is_empty()) else { continue };
            total += data.len();
            if total > MAX_ICO_BYTES {
                break;
            }
            // Width, height, colour count, reserved, planes, bit count.
            let mut head = [0; 8];
            head.copy_from_slice(&entry[..8]);
            images.push((head, data));
        }
        ico(&images)
    }
}

/// An `.ico` file from (directory entry head, image data) pairs.
fn ico(images: &[([u8; 8], Vec<u8>)]) -> Option<Vec<u8>> {
    if images.is_empty() {
        return None;
    }
    let mut offset = 6 + 16 * images.len();
    let mut out = Vec::with_capacity(offset + images.iter().map(|(_, d)| d.len()).sum::<usize>());
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&u16::try_from(images.len()).ok()?.to_le_bytes());
    for (head, data) in images {
        out.extend_from_slice(head);
        out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
        offset += data.len();
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    Some(out)
}

/// Whether `data` is an `.ico` file whose images all lie inside it.
fn is_ico(data: &[u8]) -> bool {
    let check = || -> Option<bool> {
        let count = usize::from(u16_at(data, 4)?);
        let mut ok = u16_at(data, 0)? == 0 && u16_at(data, 2)? == 1 && count > 0;
        for i in 0..count {
            let at = 6 + 16 * i;
            let size = usize::try_from(u32_at(data, at + 8)?).ok()?;
            let offset = usize::try_from(u32_at(data, at + 12)?).ok()?;
            ok &= size > 0 && offset.checked_add(size)? <= data.len();
        }
        Some(ok)
    };
    check() == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::fixtures::{self, Rng};

    fn images(tags: &[u8]) -> Vec<Vec<u8>> {
        tags.iter().map(|&t| fixtures::image(t)).collect()
    }

    #[test]
    fn extracts_icon_groups_by_index_and_id() {
        for wide in [false, true] {
            // On disk, groups sort by ID: 3 comes before 7.
            let pe = fixtures::pe(
                wide,
                &[(7, vec![(20, fixtures::image(4))]), (3, vec![(10, fixtures::image(1)), (11, fixtures::image(2))])],
            );
            assert_eq!(icon_in(&pe[..], 0), Some(fixtures::ico(&images(&[1, 2]))), "wide={wide}");
            assert_eq!(icon_in(&pe[..], 1), Some(fixtures::ico(&images(&[4]))));
            assert_eq!(icon_in(&pe[..], -7), Some(fixtures::ico(&images(&[4]))));
            assert_eq!(icon_in(&pe[..], -3), Some(fixtures::ico(&images(&[1, 2]))));
            assert_eq!(icon_in(&pe[..], 2), None);
            assert_eq!(icon_in(&pe[..], -5), None);
            assert_eq!(icon_in(&pe[..], i32::MIN), None);
        }
    }

    #[test]
    fn skips_images_that_are_missing() {
        // The group names icons 10 and 99; only 10 exists.
        let mut pe = fixtures::pe(false, &[(1, vec![(10, fixtures::image(1)), (11, fixtures::image(2))])]);
        let at = pe.windows(2).rposition(|w| w == 11u16.to_le_bytes()).expect("group entry for icon 11");
        pe[at..at + 2].copy_from_slice(&99u16.to_le_bytes());
        assert_eq!(icon_in(&pe[..], 0), Some(fixtures::ico(&images(&[1]))));
    }

    #[test]
    fn reads_ico_files_and_rejects_others() {
        let ico = fixtures::ico(&images(&[1, 2, 3]));
        assert_eq!(icon_in(&ico[..], 5), Some(ico.clone()));
        let mut bad = ico.clone();
        bad.truncate(ico.len() - 1); // the last image overruns the file
        assert_eq!(icon_in(&bad[..], 0), None);
        assert_eq!(icon_in(&fixtures::ico(&[])[..], 0), None);
        assert_eq!(icon_in(&b"\x89PNG\r\n\x1a\n"[..], 0), None);
        assert_eq!(icon_in(&b"MZ"[..], 0), None);
        assert_eq!(icon_in(&[][..], 0), None);
        // A PE without resources.
        let mut pe = fixtures::pe(true, &[(1, vec![(1, fixtures::image(1))])]);
        let resources = 0x58 + 112 + 16;
        pe[resources..resources + 4].copy_from_slice(&[0; 4]);
        assert_eq!(icon_in(&pe[..], 0), None);
    }

    #[test]
    fn reads_only_from_files_on_disk() {
        let dir = crate::test_support::TempDir::new("pe");
        let path = dir.path().join("game.exe");
        std::fs::write(&path, fixtures::pe(false, &[(1, vec![(1, fixtures::image(9))])])).unwrap();
        assert_eq!(icon(&path, 0), Some(fixtures::ico(&images(&[9]))));
        assert_eq!(icon(dir.path(), 0), None);
        assert_eq!(icon(&dir.path().join("missing.exe"), 0), None);
    }

    #[test]
    fn malformed_files_never_panic() {
        let pe = fixtures::pe(true, &[(5, vec![(1, fixtures::image(1)), (2, fixtures::image(2))]), (6, vec![])]);
        for len in 0..pe.len() {
            let _ = icon_in(&pe[..len], 0);
        }
        let mut rng = Rng(0x1234_5678);
        for _ in 0..4000 {
            let mut bytes = pe.clone();
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(bytes.len());
                bytes[at] = match rng.below(3) {
                    0 => rng.next() as u8,
                    1 => 0xff,
                    _ => 0,
                };
            }
            for index in [0, 1, -5, -6] {
                let _ = icon_in(&bytes[..], index);
            }
        }
    }
}
