#!/usr/bin/env python3
"""Extracts each Directory store's logo from its official installer.

    python3 -I tools/store-logos.py

Reads the stores (id, installer URL) from app/Sources/Crossfree/Catalog.swift
and writes app/Resources/Stores/<id>.png: the largest image of the
installer's main icon, as Explorer shows it. Only the bytes needed are
downloaded (HTTP ranges): the PE headers and resource section of an .exe;
the directory and icon streams of an .msi (a compound file).

Standard library only, plus macOS `sips` to turn BMP icon images into PNG.
"""

import re
import struct
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOG = ROOT / "app/Sources/Crossfree/Catalog.swift"
OUT = ROOT / "app/Resources/Stores"
BLOCK = 1 << 16
USER_AGENT = "crossfree-store-logos/1"


class Remote:
    """Random access to a URL through HTTP range requests, in cached blocks.
    Servers that ignore ranges are downloaded whole."""

    def __init__(self, url):
        self.url = url
        self.blocks = {}
        self.whole = None

    def read(self, offset, size):
        if self.whole is not None:
            return self.whole[offset:offset + size]
        out = bytearray()
        while size > 0:
            index, skip = divmod(offset, BLOCK)
            block = self.block(index)
            if self.whole is not None:
                return bytes(out) + self.whole[offset:offset + size]
            piece = block[skip:skip + size]
            if not piece:
                break
            out += piece
            offset += len(piece)
            size -= len(piece)
        return bytes(out)

    def block(self, index):
        if index not in self.blocks:
            start = index * BLOCK
            request = urllib.request.Request(
                self.url,
                headers={"Range": f"bytes={start}-{start + BLOCK - 1}", "User-Agent": USER_AGENT},
            )
            with urllib.request.urlopen(request, timeout=60) as response:
                data = response.read()
                if response.status == 200:  # no ranges: this is the whole file
                    self.whole = data
                    return data
            self.blocks[index] = data
        return self.blocks[index]


class Local:
    def __init__(self, data):
        self.data = data

    def read(self, offset, size):
        return self.data[offset:offset + size]


def u16(reader, offset):
    return struct.unpack("<H", reader.read(offset, 2))[0]


def u32(reader, offset):
    return struct.unpack("<I", reader.read(offset, 4))[0]


# --- PE resources --------------------------------------------------------

def pe_icon_images(reader):
    """The images of the first RT_GROUP_ICON (icon index 0), as
    (width, bits, bytes)."""
    if reader.read(0, 2) != b"MZ":
        return []
    pe = u32(reader, 0x3C)
    if reader.read(pe, 4) != b"PE\0\0":
        return []
    sections = u16(reader, pe + 6)
    optional = pe + 24
    optional_size = u16(reader, pe + 20)
    magic = u16(reader, optional)
    directories = optional + (96 if magic == 0x10B else 112)
    resource_rva = u32(reader, directories + 2 * 8)
    if resource_rva == 0:
        return []
    table = []
    for i in range(sections):
        s = optional + optional_size + 40 * i
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack("<IIII", reader.read(s + 8, 16))
        table.append((virtual_address, max(virtual_size, raw_size), raw_offset))

    def offset(rva):
        for address, size, raw in table:
            if address <= rva < address + size:
                return raw + rva - address
        raise ValueError(f"RVA {rva:#x} is in no section")

    base = offset(resource_rva)

    def entries(directory):
        named, ids = struct.unpack("<HH", reader.read(directory + 12, 4))
        for i in range(named + ids):
            name, target = struct.unpack("<II", reader.read(directory + 16 + 8 * i, 8))
            yield name, target

    def child(directory, wanted):
        for name, target in entries(directory):
            if not name & 0x80000000 and name == wanted:
                return target
        return None

    def first_data(target):
        # Down the first branch (name, then language) to the data.
        while target & 0x80000000:
            target = next(entries(base + (target & 0x7FFFFFFF)))[1]
        rva, size = struct.unpack("<II", reader.read(base + target, 8))
        return reader.read(offset(rva), size)

    groups = child(base, 14)
    icons = child(base, 3)
    if groups is None or icons is None or not groups & 0x80000000:
        return []
    first_group = next(entries(base + (groups & 0x7FFFFFFF)))[1]
    group = first_data(first_group)
    count = struct.unpack("<H", group[4:6])[0]
    images = []
    for i in range(count):
        width, _height, _colors, _reserved, _planes, bits, _size, icon_id = struct.unpack(
            "<BBBBHHIH", group[6 + 14 * i:6 + 14 * (i + 1)]
        )
        target = child(base + (icons & 0x7FFFFFFF), icon_id)
        if target is not None:
            images.append((width or 256, bits, first_data(target)))
    return images


def ico_images(data):
    if data[:4] != b"\0\0\1\0":
        return []
    count = struct.unpack("<H", data[4:6])[0]
    images = []
    for i in range(count):
        width, _h, _c, _r, _p, bits, size, start = struct.unpack("<BBBBHHII", data[6 + 16 * i:6 + 16 * (i + 1)])
        images.append((width or 256, bits, data[start:start + size]))
    return images


# --- MSI (compound file) -------------------------------------------------

MSI_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz._"


def msi_name(raw):
    """Decodes an MSI stream name (two characters packed per code unit)."""
    out = []
    for ch in raw:
        code = ord(ch)
        if 0x3800 <= code < 0x4800:
            code -= 0x3800
            out.append(MSI_ALPHABET[code & 0x3F])
            out.append(MSI_ALPHABET[(code >> 6) & 0x3F])
        elif 0x4800 <= code < 0x4840:
            out.append(MSI_ALPHABET[code - 0x4800])
        elif code == 0x4840:
            out.append("!")
        else:
            out.append(ch)
    return "".join(out)


def msi_icon_images(reader):
    """The images of the MSI's Icon table entries (each an .ico or .exe)."""
    if reader.read(0, 8) != bytes.fromhex("D0CF11E0A1B11AE1"):
        return []
    sector = 1 << u16(reader, 0x1E)
    mini = 1 << u16(reader, 0x20)
    first_directory = u32(reader, 0x30)
    cutoff = u32(reader, 0x38)
    first_minifat = u32(reader, 0x3C)
    first_difat = u32(reader, 0x44)
    difat = list(struct.unpack("<109I", reader.read(0x4C, 436)))
    next_difat = first_difat
    while next_difat < 0xFFFFFFFA:
        words = struct.unpack(f"<{sector // 4}I", reader.read((next_difat + 1) * sector, sector))
        difat += words[:-1]
        next_difat = words[-1]
    per_sector = sector // 4

    def fat(n):
        fat_sector = difat[n // per_sector]
        return u32(reader, (fat_sector + 1) * sector + 4 * (n % per_sector))

    def chain(start):
        seen = set()
        while start < 0xFFFFFFFA and start not in seen:
            seen.add(start)
            yield start
            start = fat(start)

    def stream(start, size):
        data = bytearray()
        for n in chain(start):
            data += reader.read((n + 1) * sector, sector)
            if len(data) >= size:
                break
        return bytes(data[:size])

    directory = b"".join(reader.read((n + 1) * sector, sector) for n in chain(first_directory))
    entries = []
    for i in range(len(directory) // 128):
        entry = directory[128 * i:128 * (i + 1)]
        name_length = struct.unpack("<H", entry[64:66])[0]
        kind = entry[66]
        start, size = struct.unpack("<IQ", entry[116:128])
        name = entry[:max(name_length - 2, 0)].decode("utf-16-le", "replace")
        entries.append((name, kind, start, size))
    root = next(e for e in entries if e[1] == 5)
    ministream = None
    minifat = b"".join(reader.read((n + 1) * sector, sector) for n in chain(first_minifat))

    def mini_stream(start, size):
        nonlocal ministream
        if ministream is None:
            ministream = stream(root[2], root[3])
        data = bytearray()
        seen = set()
        while start < 0xFFFFFFFA and start not in seen and len(data) < size:
            seen.add(start)
            data += ministream[start * mini:(start + 1) * mini]
            start = struct.unpack("<I", minifat[4 * start:4 * start + 4])[0]
        return bytes(data[:size])

    images = []
    for name, kind, start, size in entries:
        if kind != 2 or not msi_name(name).startswith("Icon."):
            continue
        data = mini_stream(start, size) if size < cutoff else stream(start, size)
        images += ico_images(data) or pe_icon_images(Local(data))
    return images


# --- Output --------------------------------------------------------------

def best(images):
    return max(images, key=lambda image: (image[0], image[1], len(image[2])))


def write_png(image, path):
    width, bits, data = image
    if data[:8] == b"\x89PNG\r\n\x1a\n":
        path.write_bytes(data)
        return
    # A BMP image: wrap it in a one-image .ico, which sips can convert.
    header = struct.pack("<HHH", 0, 1, 1) + struct.pack(
        "<BBBBHHII", width % 256, width % 256, 0, 0, 1, bits, len(data), 22
    )
    with tempfile.TemporaryDirectory() as tmp:
        ico = Path(tmp) / "icon.ico"
        ico.write_bytes(header + data)
        subprocess.run(["sips", "-s", "format", "png", str(ico), "--out", str(path)],
                       check=True, stdout=subprocess.DEVNULL)


def main():
    text = CATALOG.read_text()
    stores = re.findall(r'id: "([a-z0-9]+)".*?installer: URL\(string: "([^"]+)"\)', text, re.DOTALL)
    if not stores:
        sys.exit(f"no stores found in {CATALOG}")
    OUT.mkdir(parents=True, exist_ok=True)
    failed = 0
    for store_id, url in stores:
        reader = Remote(url)
        try:
            images = msi_icon_images(reader) or pe_icon_images(reader)
        # A failed download (URLError is an OSError) or a malformed file:
        # report every store, then fail.
        except (OSError, ValueError, struct.error, IndexError, StopIteration) as error:
            images = []
            print(f"{store_id}: {error}", file=sys.stderr)
        if not images:
            print(f"{store_id}: no icon found", file=sys.stderr)
            failed += 1
            continue
        image = best(images)
        path = OUT / f"{store_id}.png"
        write_png(image, path)
        fetched = len(reader.whole) if reader.whole is not None else BLOCK * len(reader.blocks)
        print(f"{store_id}: {image[0]}px, {path.relative_to(ROOT)} ({fetched // 1024} KB fetched)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
