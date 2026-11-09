//! Test data: synthetic executables, icon files and shortcuts.

/// An icon image's bytes; `tag` makes each one different.
pub(crate) fn image(tag: u8) -> Vec<u8> {
    vec![tag; 40 + usize::from(tag)]
}

/// The directory entry head (width, height, colours, reserved, planes, bit
/// count) the fixtures give every image: 16×16, 32-bit.
const HEAD: [u8; 8] = [16, 16, 0, 0, 1, 0, 32, 0];

/// An `.ico` file holding `images`.
pub(crate) fn ico(images: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for data in images {
        out.extend_from_slice(&HEAD);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for data in images {
        out.extend_from_slice(data);
    }
    out
}

/// An icon group: its id and its `(icon id, image)` entries.
pub(crate) type IconGroup = (u16, Vec<(u16, Vec<u8>)>);

/// A resource type: its id (`None` for a named type) and its
/// `(name id, data)` entries.
type ResourceType = (Option<u32>, Vec<(u32, Vec<u8>)>);

/// A PE file (PE32+ if `wide`) whose resources hold the icon groups
/// `(group id, [(icon id, image)])`. Like many real programs, it also has a
/// named resource type, which sorts first.
pub(crate) fn pe(wide: bool, groups: &[IconGroup]) -> Vec<u8> {
    const SECTION_VA: u32 = 0x2000;
    const SECTION_OFFSET: usize = 0x400;

    let mut groups: Vec<_> = groups.to_vec();
    groups.sort_by_key(|(id, _)| *id);
    let mut icons: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut group_data = Vec::new();
    for (group, images) in &groups {
        let mut dir = vec![0, 0, 1, 0];
        dir.extend_from_slice(&(images.len() as u16).to_le_bytes());
        for (id, data) in images {
            dir.extend_from_slice(&HEAD);
            dir.extend_from_slice(&(data.len() as u32).to_le_bytes());
            dir.extend_from_slice(&id.to_le_bytes());
            icons.push((u32::from(*id), data.clone()));
        }
        group_data.push((u32::from(*group), dir));
    }
    icons.sort_by_key(|(id, _)| *id);
    // (type id, None for the named type; resources)
    let types: Vec<ResourceType> = vec![(None, vec![(1, b"named".to_vec())]), (Some(3), icons), (Some(14), group_data)];
    let resources = resource_section(SECTION_VA, &types);

    let mut out = vec![0; SECTION_OFFSET];
    out[..2].copy_from_slice(b"MZ");
    out[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    out[0x40..0x44].copy_from_slice(b"PE\0\0");
    let machine: u16 = if wide { 0x8664 } else { 0x14c };
    out[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
    out[0x46..0x48].copy_from_slice(&1u16.to_le_bytes()); // sections
    let optional_size: u16 = if wide { 0xf0 } else { 0xe0 };
    out[0x54..0x56].copy_from_slice(&optional_size.to_le_bytes());
    let optional = 0x58;
    let (magic, count_at, directories_at): (u16, usize, usize) = if wide { (0x20b, 108, 112) } else { (0x10b, 92, 96) };
    out[optional..optional + 2].copy_from_slice(&magic.to_le_bytes());
    out[optional + count_at..optional + count_at + 4].copy_from_slice(&16u32.to_le_bytes());
    let resource_dir = optional + directories_at + 16;
    out[resource_dir..resource_dir + 4].copy_from_slice(&SECTION_VA.to_le_bytes());
    out[resource_dir + 4..resource_dir + 8].copy_from_slice(&(resources.len() as u32).to_le_bytes());
    let section = optional + usize::from(optional_size);
    out[section..section + 8].copy_from_slice(b".rsrc\0\0\0");
    out[section + 8..section + 12].copy_from_slice(&(resources.len() as u32).to_le_bytes());
    out[section + 12..section + 16].copy_from_slice(&SECTION_VA.to_le_bytes());
    out[section + 16..section + 20].copy_from_slice(&(resources.len() as u32).to_le_bytes());
    out[section + 20..section + 24].copy_from_slice(&(SECTION_OFFSET as u32).to_le_bytes());
    out.extend_from_slice(&resources);
    out
}

/// A resource section at `va`: type → name → language (1033) → data.
fn resource_section(va: u32, types: &[ResourceType]) -> Vec<u8> {
    // Layout: every directory, then data entries, the type name, the data.
    let mut size = 0;
    let mut alloc = |n: usize| {
        let at = size;
        size += n;
        at
    };
    alloc(16 + 8 * types.len()); // the root directory
    let type_dirs: Vec<usize> = types.iter().map(|(_, r)| alloc(16 + 8 * r.len())).collect();
    let language_dirs: Vec<Vec<usize>> = types.iter().map(|(_, r)| r.iter().map(|_| alloc(24)).collect()).collect();
    let data_entries: Vec<Vec<usize>> = types.iter().map(|(_, r)| r.iter().map(|_| alloc(16)).collect()).collect();
    let name = alloc(10);
    let blobs: Vec<Vec<usize>> =
        types.iter().map(|(_, r)| r.iter().map(|(_, d)| alloc(d.len().next_multiple_of(4))).collect()).collect();
    let total = alloc(0);

    let mut out = vec![0u8; total];
    let mut put = |at: usize, value: u32| out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    let named = types.iter().filter(|(t, _)| t.is_none()).count() as u32;
    put(12, named | ((types.len() as u32 - named) << 16));
    for (i, (ty, resources)) in types.iter().enumerate() {
        put(16 + 8 * i, ty.unwrap_or(0x8000_0000 | name as u32));
        put(20 + 8 * i, 0x8000_0000 | type_dirs[i] as u32);
        put(type_dirs[i] + 12, (resources.len() as u32) << 16);
        for (j, (id, data)) in resources.iter().enumerate() {
            put(type_dirs[i] + 16 + 8 * j, *id);
            put(type_dirs[i] + 20 + 8 * j, 0x8000_0000 | language_dirs[i][j] as u32);
            put(language_dirs[i][j] + 12, 1 << 16);
            put(language_dirs[i][j] + 16, 1033);
            put(language_dirs[i][j] + 20, data_entries[i][j] as u32);
            put(data_entries[i][j], va + blobs[i][j] as u32);
            put(data_entries[i][j] + 4, data.len() as u32);
        }
    }
    out[name..name + 2].copy_from_slice(&4u16.to_le_bytes());
    for (k, unit) in "SCID".encode_utf16().enumerate() {
        out[name + 2 + 2 * k..name + 4 + 2 * k].copy_from_slice(&unit.to_le_bytes());
    }
    for (i, (_, resources)) in types.iter().enumerate() {
        for (j, (_, data)) in resources.iter().enumerate() {
            out[blobs[i][j]..blobs[i][j] + data.len()].copy_from_slice(data);
        }
    }
    out
}

/// A Unicode `.lnk` file with an ID list and a name. `target` goes in
/// LinkInfo as the local base path; `icon` is the icon location.
pub(crate) fn lnk(target: Option<&str>, icon: Option<&str>, index: i32) -> Vec<u8> {
    let mut flags = 1 | 1 << 2 | 1 << 7; // ID list, name, Unicode
    flags |= u32::from(target.is_some()) << 1 | u32::from(icon.is_some()) << 6;
    let mut out = vec![0; 0x4c];
    out[..4].copy_from_slice(&0x4cu32.to_le_bytes());
    out[4..20].copy_from_slice(&[0x01, 0x14, 0x02, 0x00, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
    out[20..24].copy_from_slice(&flags.to_le_bytes());
    out[56..60].copy_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[4, 0, 2, 0, 0, 0]); // ID list: one empty item, terminator
    if let Some(target) = target {
        let header = 0x1c;
        let suffix = header + target.len() + 1;
        let size = suffix + 1;
        for value in [size, header, 1, 0, header, 0, suffix] {
            out.extend_from_slice(&(value as u32).to_le_bytes());
        }
        out.extend_from_slice(target.as_bytes());
        out.extend_from_slice(&[0, 0]);
    }
    for text in ["A game"].into_iter().chain(icon) {
        out.extend_from_slice(&(text.encode_utf16().count() as u16).to_le_bytes());
        out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    }
    out.extend_from_slice(&[0; 4]); // terminal extra-data block
    out
}

/// A small deterministic random generator (xorshift) for mutation tests.
pub(crate) struct Rng(pub u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
