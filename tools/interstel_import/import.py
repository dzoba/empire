#!/usr/bin/env python3
"""Import the data files of Interstel's "Empire: Wargame of the Century" (DOS, v2.05).

Usage:
    python3 tools/interstel_import/import.py <EMPDAT dir> [--out assets/original]

Reads the original EMPDAT directory (EMPICON*.CGA/EGA, *.MAP, XTRAMAPS/*.MAP,
CGAFNT.FY, GOTHIC1.ESI, EMPIRE1/2.PCX) and writes plain-text / simple binary
assets that the Rust game parses.  The formats on both sides are documented in
docs/interstel-formats.md.

Game-facing outputs (.tiles, .emap, .font, .widths, .img) use only the Python
standard library.  PNG previews (contact sheets, title pictures, map preview,
font sheets) need Pillow; without it they are skipped with a warning.

Re-running overwrites the same set of files with identical content, so the
script is idempotent.  It never deletes anything else in the output directory.
"""

import argparse
import os
import sys
from collections import Counter

try:
    from PIL import Image, ImageDraw
except ImportError:  # PNG previews are optional
    Image = None
    ImageDraw = None

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEFAULT_OUT = os.path.join(REPO_ROOT, "assets", "original")

ROWS, COLS = 60, 100
MAPSIZE = ROWS * COLS

# CGA palette 1, high intensity.  Index = pixel value in the files.
CGA_RGB = [(0, 0, 0), (85, 255, 255), (255, 85, 255), (255, 255, 255)]
# Standard EGA 16-colour palette (IBM order: plane0=blue, 1=green, 2=red, 3=intensity).
EGA_RGB = [
    (0, 0, 0), (0, 0, 170), (0, 170, 0), (0, 170, 170),
    (170, 0, 0), (170, 0, 170), (170, 85, 0), (170, 170, 170),
    (85, 85, 85), (85, 85, 255), (85, 255, 85), (85, 255, 255),
    (255, 85, 85), (255, 85, 255), (255, 255, 85), (255, 255, 255),
]

# --------------------------------------------------------------------------
# Icon naming.  Same 65-slot layout in every EMPICON file (CGA and EGA).
# --------------------------------------------------------------------------

UNIT_BLOCK = ["city", "army", "fighter", "fighter_sea", "destroyer",
              "transport", "submarine", "cruiser", "carrier", "battleship"]

ICON_DESCRIPTIONS = {
    "unknown": "unexplored square (solid black)",
    "city_neutral": "neutral (unowned) city: black grid on white, the inverse of the owned city",
    "sea": "open sea",
    "land": "land with no orthogonal sea neighbour (coast mask 0)",
    "city": "owned city (grid of player-colour lines on black)",
    "army": "army (tank silhouette)",
    "fighter": "fighter over land (CGA: dotted land-texture L along left/bottom edge; EGA: land background)",
    "fighter_sea": "fighter over sea (CGA: solid sea-colour L along left/bottom edge; EGA: sea background)",
    "destroyer": "destroyer (smallest hull)",
    "transport": "troop transport, empty (EGA: smoke puffs)",
    "submarine": "submarine (flat hull with conning tower)",
    "cruiser": "cruiser",
    "carrier": "aircraft carrier, empty deck",
    "battleship": "battleship (largest, most turrets)",
    "border": "inaccessible map edge (fine black/white checkerboard, reads as grey)",
    "cursor_direction": "probably the Go Direction cursor (corner blocks around a centre dot); see docs",
    "cursor_survey": "probably the survey-mode cursor (corner brackets, hollow centre); see docs",
    "explosion": "combat explosion (mixed colours)",
    "city_occupied": "owned city with units inside (denser 'shaded' grid, corners filled)",
    "army_sentry": "army on sentry duty (tent)",
    "transport_loaded": "troop transport with armies aboard (portholes)",
    "carrier_loaded": "aircraft carrier with fighters on deck",
}


def icon_names():
    names = ["unknown", "city_neutral", "sea", "land"]
    for p in (1, 2, 3):
        names += ["%s_p%d" % (u, p) for u in UNIT_BLOCK]
    names += ["border", "cursor_direction", "cursor_survey", "explosion"]
    for base in ("city_occupied", "army_sentry", "transport_loaded", "carrier_loaded"):
        names += ["%s_p%d" % (base, p) for p in (1, 2, 3)]
    names += ["land_coast_%d" % m for m in range(1, 16)]
    assert len(names) == 65
    return names


def coast_mask_name(mask):
    return "land" if mask == 0 else "land_coast_%d" % mask


def describe_icon(name):
    if name.startswith("land_coast_"):
        m = int(name.rsplit("_", 1)[1])
        sides = [s for bit, s in ((1, "N"), (2, "E"), (4, "S"), (8, "W")) if m & bit]
        return "land with coast line on the %s edge%s (mask %d)" % (
            "+".join(sides), "s" if len(sides) > 1 else "", m)
    base, player = name, None
    if name[-3:-1] == "_p":
        base, player = name[:-3], name[-1]
    d = ICON_DESCRIPTIONS.get(base, "")
    return d + (", player %s" % player if player else "")


# --------------------------------------------------------------------------
# Small helpers
# --------------------------------------------------------------------------

def u16le(b, o):
    return b[o] | (b[o + 1] << 8)


def write_text(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", newline="\n") as f:
        f.write(text)


def write_bytes(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def find_file(dirpath, name):
    """Case-insensitive lookup of a file inside dirpath."""
    if not os.path.isdir(dirpath):
        return None
    want = name.lower()
    for entry in sorted(os.listdir(dirpath)):
        if entry.lower() == want:
            return os.path.join(dirpath, entry)
    return None


# --------------------------------------------------------------------------
# Icon files (EMPICONn.CGA / EMPICONn.EGA)
# --------------------------------------------------------------------------

def parse_icons(path):
    """Header: u16 count, u16 bytes per record, u16 width, u16 height (pixels),
    then 4 bytes of colour index per player slot 0..3.  Each record starts with
    u16 bits-per-row-per-plane and u16 rows, then the bitmap."""
    b = open(path, "rb").read()
    count, recsize, w, h = (u16le(b, i) for i in (0, 2, 4, 6))
    colours = list(b[8:12])
    if 12 + count * recsize != len(b):
        raise ValueError("%s: size %d != 12 + %d*%d" % (path, len(b), count, recsize))
    data_bytes = recsize - 4
    if data_bytes == w * h * 2 // 8:
        kind, planes = "cga", 1          # 2 bits per pixel, packed
    elif data_bytes == w * h * 4 // 8:
        kind, planes = "ega", 4          # 4 bit planes, row-interleaved
    else:
        raise ValueError("%s: unknown icon layout" % path)
    tiles = []
    for t in range(count):
        o = 12 + t * recsize
        bits_per_row, rows = u16le(b, o), u16le(b, o + 2)
        if rows != h:
            raise ValueError("%s tile %d: %d rows, expected %d" % (path, t, rows, h))
        d = b[o + 4:o + recsize]
        px = []
        if kind == "cga":
            if bits_per_row != w * 2:
                raise ValueError("%s tile %d: bits/row %d" % (path, t, bits_per_row))
            bpr = w // 4
            for r in range(h):
                px.append([(d[r * bpr + c // 4] >> (6 - 2 * (c % 4))) & 3 for c in range(w)])
        else:
            if bits_per_row != w:
                raise ValueError("%s tile %d: bits/row %d" % (path, t, bits_per_row))
            bpp = w // 8            # bytes per plane-row
            for r in range(h):
                base = r * bpp * 4
                pl = [int.from_bytes(d[base + p * bpp:base + (p + 1) * bpp], "big") for p in range(4)]
                px.append([sum(((pl[p] >> (w - 1 - c)) & 1) << p for p in range(4)) for c in range(w)])
        tiles.append(px)
    return {"kind": kind, "count": count, "w": w, "h": h, "colours": colours, "tiles": tiles,
            "path": path}


def write_tiles(path, info, names, setno):
    kind = info["kind"]
    lines = [
        "# Interstel Empire v2.05 icon set: %s" % os.path.basename(info["path"]),
        "# Generated by tools/interstel_import/import.py; see docs/interstel-formats.md",
        "# %d tiles of %dx%d pixels, in file order" % (info["count"], info["w"], info["h"]),
        "# header player colour table (bytes 8-11, slots 0..3): %s" % " ".join(str(c) for c in info["colours"]),
        "# coast tiles: land_coast_<m>, m = sea-neighbour mask N=1 E=2 S=4 W=8 (orthogonal only); m=0 is 'land'",
    ]
    if kind == "cga":
        lines.append("# 0=black 1=cyan 2=magenta 3=white (CGA palette 1, high intensity)")
        lines.append("palette cga1")
        digits = "0123"
    else:
        lines.append("# EGA tiles are 16x8 pixels for the 640x200 16-colour mode (non-square pixels)")
        lines.append("palette ega")
        digits = "0123456789abcdef"
    lines.append("")
    for i, (name, px) in enumerate(zip(names, info["tiles"])):
        lines.append("# index %02d" % i)
        if info["w"] == 8 and info["h"] == 8:
            lines.append("tile %s" % name)
        else:
            lines.append("tile %s %d %d" % (name, info["w"], info["h"]))
        for row in px:
            lines.append("".join(digits[v] for v in row))
        lines.append("")
    write_text(path, "\n".join(lines))


def coast_band_check(info):
    """Infer which edges each coast tile (50..64) changes relative to plain land
    (tile 3), using the middle half of a 2-pixel (CGA) / 4-pixel-wide (EGA) band
    along each edge.  Returns list of (index, expected_mask, detected_mask)."""
    T = info["tiles"]
    base = T[3]
    h, w = info["h"], info["w"]
    res = []
    for t in range(50, 65):
        def ch(r, c):
            return T[t][r][c] != base[r][c]
        mid_c = range(w // 4, w - w // 4)
        mid_r = range(h // 4, h - h // 4)
        side = 2 if w == 8 else 4
        frac = {
            1: sum(ch(r, c) for r in (0, 1) for c in mid_c) / (2.0 * len(mid_c)),
            4: sum(ch(r, c) for r in (h - 2, h - 1) for c in mid_c) / (2.0 * len(mid_c)),
            2: sum(ch(r, c) for c in range(w - side, w) for r in mid_r) / float(side * len(mid_r)),
            8: sum(ch(r, c) for c in range(0, side) for r in mid_r) / float(side * len(mid_r)),
        }
        res.append((t, t - 49, sum(k for k, v in frac.items() if v > 0.2)))
    return res


def coast_colour_check(info):
    """Alternative check for sets whose coast is drawn as sea-coloured pixels on
    the outermost line (CGA set 3).  Coast colour = most common colour among
    changed pixels; an edge counts when its outer line gains >= 2 such pixels."""
    T = info["tiles"]
    base = T[3]
    h, w = info["h"], info["w"]
    cnt = Counter(T[t][r][c] for t in range(50, 65) for r in range(h) for c in range(w)
                  if T[t][r][c] != base[r][c])
    cc = cnt.most_common(1)[0][0]

    def line(px, e):
        if e == 1:
            return [px[0][c] for c in range(1, w - 1)]
        if e == 4:
            return [px[h - 1][c] for c in range(1, w - 1)]
        if e == 2:
            return [px[r][w - 1] for r in range(1, h - 1)]
        return [px[r][0] for r in range(1, h - 1)]
    res = []
    for t in range(50, 65):
        m = 0
        for e in (1, 2, 4, 8):
            if line(T[t], e).count(cc) - line(base, e).count(cc) >= 2:
                m |= e
        res.append((t, t - 49, m))
    return cc, res


def icon_sheet(info, names, path, sx, sy, title):
    if Image is None:
        return False
    pal = CGA_RGB if info["kind"] == "cga" else EGA_RGB
    tw, th = info["w"] * sx, info["h"] * sy
    cols = 10 if info["kind"] == "cga" else 8
    cellw = max(tw, 136) + 8
    cellh = th + 26
    rows = (info["count"] + cols - 1) // cols
    im = Image.new("RGB", (cols * cellw + 8, rows * cellh + 26), (64, 64, 64))
    d = ImageDraw.Draw(im)
    d.text((6, 6), title, fill=(255, 255, 255))
    for i, px in enumerate(info["tiles"]):
        x0 = 8 + (i % cols) * cellw
        y0 = 26 + (i // cols) * cellh
        d.text((x0, y0), "%02d %s" % (i, names[i][:20]), fill=(255, 255, 0))
        for r in range(info["h"]):
            for c in range(info["w"]):
                x, y = x0 + c * sx, y0 + 11 + r * sy
                d.rectangle([x, y, x + sx - 1, y + sy - 1], fill=pal[px[r][c]])
    im.save(path)
    return True


# --------------------------------------------------------------------------
# Maps (*.MAP)
# --------------------------------------------------------------------------

def parse_map(path):
    """4-byte header (row max 59, col max 99 as u16; big-endian in A-J and the
    extra maps, little-endian in K and L), RLE terrain stream filled from cell
    5999 down to 0 and terminated by a 0 byte, then one big-endian u16 per city
    in ascending cell order."""
    b = open(path, "rb").read()
    hdr = b[0:4]
    if hdr == bytes([0, 59, 0, 99]):
        endian = "big"
    elif hdr == bytes([59, 0, 99, 0]):
        endian = "little"
    else:
        endian = "unknown"
    cells = [0] * MAPSIZE
    i, k = 4, MAPSIZE - 1
    while True:
        if i >= len(b):
            raise ValueError("%s: missing RLE terminator" % path)
        c = b[i]
        i += 1
        if c == 0:
            break
        n, a = ((c >> 2) & 63) + 1, c & 3
        if a == 0 or k - n + 1 < 0:
            raise ValueError("%s: bad RLE byte %02x at %d" % (path, c, i - 1))
        for _ in range(n):
            cells[k] = a
            k -= 1
    if k != -1:
        raise ValueError("%s: RLE filled only %d cells" % (path, MAPSIZE - 1 - k))
    rle_bytes = i - 5
    rest = b[i:]
    cities = [j for j in range(MAPSIZE) if cells[j] == 1]
    if len(rest) != 2 * len(cities):
        raise ValueError("%s: %d trailing bytes for %d cities" % (path, len(rest), len(cities)))
    vals = [(rest[2 * j] << 8) | rest[2 * j + 1] for j in range(len(cities))]
    return {"path": path, "header": hdr, "endian": endian, "cells": cells, "cities": cities,
            "city_vals": vals, "rle_bytes": rle_bytes, "size": len(b)}


TERRAIN_CHAR = {1: "*", 2: ".", 3: "+"}


def ring_cells():
    for c in range(COLS):
        yield c
        yield (ROWS - 1) * COLS + c
    for r in range(1, ROWS - 1):
        yield r * COLS
        yield r * COLS + COLS - 1


def ring_summary(cells):
    cnt = Counter(TERRAIN_CHAR[cells[i]] for i in set(ring_cells()))
    return " ".join("%s=%d" % (k, cnt[k]) for k in sorted(cnt))


def landmass_city_counts(cells):
    """8-connected land+city components, ignoring the outer ring (K and L store
    it as land, which would join every coast); returns (component per cell,
    city count per component)."""
    comp = [-1] * MAPSIZE
    for i in ring_cells():
        comp[i] = -2                     # never part of a landmass
    ncity = []
    for s in range(MAPSIZE):
        if cells[s] in (1, 3) and comp[s] == -1:
            cid = len(ncity)
            comp[s] = cid
            stack, nc = [s], 0
            while stack:
                x = stack.pop()
                nc += cells[x] == 1
                r, c = divmod(x, COLS)
                for dr in (-1, 0, 1):
                    for dc in (-1, 0, 1):
                        rr, cc = r + dr, c + dc
                        if 0 <= rr < ROWS and 0 <= cc < COLS:
                            y = rr * COLS + cc
                            if cells[y] in (1, 3) and comp[y] == -1:
                                comp[y] = cid
                                stack.append(y)
            ncity.append(nc)
    return comp, ncity


def write_emap(path, m, name):
    cells = m["cells"]
    lines = [
        "# Interstel Empire v2.05 map %s" % name,
        "# Generated by tools/interstel_import/import.py; see docs/interstel-formats.md",
        "# %d columns x %d rows; '.' sea, '+' land, '*' city; row 0 is the top line" % (COLS, ROWS),
        "# file header bytes %s (%s-endian row max / col max)" % (m["header"].hex(" "), m["endian"]),
        "# outer ring as stored: %s (the game treats the ring as the inaccessible border)" % ring_summary(cells),
        "# cities: %d" % len(m["cities"]),
        "# city <row> <col> <aux>: aux = the u16 stored per city after the terrain stream,",
        "#   in ascending cell order. Meaning not fully confirmed: it matches the number of",
        "#   cities on the same 8-connected landmass in most standard maps; see the docs.",
    ]
    for ci, v in zip(m["cities"], m["city_vals"]):
        r, c = divmod(ci, COLS)
        lines.append("# city %d %d %d" % (r, c, v))
    for r in range(ROWS):
        lines.append("".join(TERRAIN_CHAR[cells[r * COLS + c]] for c in range(COLS)))
    write_text(path, "\n".join(lines) + "\n")


def coast_mask(cells, i):
    r, c = divmod(i, COLS)
    m = 0
    if r > 0 and cells[i - COLS] == 2:
        m |= 1
    if c < COLS - 1 and cells[i + 1] == 2:
        m |= 2
    if r < ROWS - 1 and cells[i + COLS] == 2:
        m |= 4
    if c > 0 and cells[i - 1] == 2:
        m |= 8
    return m


def render_map(m, info, names, path, scale=1):
    if Image is None:
        return False
    idx = {n: i for i, n in enumerate(names)}
    cells = m["cells"]
    tw, th = info["w"], info["h"]
    pal = CGA_RGB if info["kind"] == "cga" else EGA_RGB
    im = Image.new("RGB", (COLS * tw, ROWS * th))
    pix = im.load()
    ring = set(ring_cells())
    for i in range(MAPSIZE):
        r, c = divmod(i, COLS)
        if i in ring:
            t = idx["border"]
        elif cells[i] == 1:
            t = idx["city_neutral"]
        elif cells[i] == 2:
            t = idx["sea"]
        else:
            t = idx[coast_mask_name(coast_mask(cells, i))]
        px = info["tiles"][t]
        for y in range(th):
            for x in range(tw):
                pix[c * tw + x, r * th + y] = pal[px[y][x]]
    if scale != 1:
        im = im.resize((im.width * scale, im.height * scale), Image.NEAREST)
    im.save(path)
    return True


# --------------------------------------------------------------------------
# Fonts: CGAFNT.FY and GOTHIC1.ESI
# --------------------------------------------------------------------------

# Both font files are XOR-obfuscated with this 14-byte string, each byte with bit 7 set.
FONT_KEY = bytes(ch | 0x80 for ch in b"(C)1984WKMASON")


def xor_key(data, key_start):
    return bytes(v ^ FONT_KEY[(i + key_start) % len(FONT_KEY)] for i, v in enumerate(data))


def unrle55(d):
    """0x55 is an escape: 55 <count> <value> expands to <count> copies of <value>."""
    out = bytearray()
    i = 0
    while i < len(d):
        if d[i] == 0x55 and i + 2 < len(d):
            out += bytes([d[i + 2]]) * d[i + 1]
            i += 3
        else:
            out.append(d[i])
            i += 1
    return bytes(out)


def parse_fy(path):
    """CGAFNT.FY: whole file XORed with FONT_KEY from offset 0, then 0x55-RLE.
    Decoded: 8-byte header (byte0 = 4 columns per cell, byte4 = 94 glyphs,
    byte7 = 8 rows), 94 glyphs for '!'..'~' of 4 column bytes each (bit 7 = top
    row), then 94 advance widths (columns actually used)."""
    raw = open(path, "rb").read()
    d = unrle55(xor_key(raw, 0))
    cellw, count, height = d[0], d[4], d[7]
    if (cellw, count, height) != (4, 94, 8) or len(d) != 8 + count * cellw + count:
        raise ValueError("%s: unexpected decoded layout (%d,%d,%d,len %d)" % (path, cellw, count, height, len(d)))
    glyphs, widths = {}, {}
    wtab = d[8 + count * cellw:]
    for k in range(count):
        cols = d[8 + k * cellw:8 + (k + 1) * cellw]
        cp = 0x21 + k
        glyphs[cp] = [[(cols[c] >> (7 - r)) & 1 for c in range(cellw)] for r in range(height)]
        widths[cp] = wtab[k]
    return {"w": cellw, "h": height, "glyphs": glyphs, "widths": widths, "header": d[:8]}


def parse_esi(path):
    """GOTHIC1.ESI: 12-byte clear header (byte1 = 95 glyphs, u16@6 = 3 bytes per
    row, u16@8 = 72 bytes per glyph), 95 clear advance widths for ' '..'~',
    u16 10 (probably the space width), then 95 glyph bitmaps of 24 rows x 3
    bytes for '!'..0x7F, XORed with FONT_KEY starting at key index 0."""
    raw = open(path, "rb").read()
    count, bpr, bpg = raw[1], u16le(raw, 6), u16le(raw, 8)
    wstart = 12
    widths_tab = raw[wstart:wstart + count]
    space_w = u16le(raw, wstart + count)
    dstart = wstart + count + 2
    data = xor_key(raw[dstart:], 0)
    if len(data) != count * bpg:
        raise ValueError("%s: %d bitmap bytes, expected %d" % (path, len(data), count * bpg))
    rows, w = bpg // bpr, bpr * 8
    glyphs, widths = {}, {}
    for k in range(count):
        cp = 0x21 + k
        g = data[k * bpg:(k + 1) * bpg]
        glyphs[cp] = [[(g[r * bpr + c // 8] >> (7 - c % 8)) & 1 for c in range(w)] for r in range(rows)]
    for k in range(count):
        widths[0x20 + k] = widths_tab[k]
    widths[0x20] = space_w
    return {"w": w, "h": rows, "glyphs": glyphs, "widths": widths, "header": raw[:12]}


def write_font(path, font, codepoints):
    lines = ["font %d %d" % (font["w"], font["h"])]
    for cp in codepoints:
        lines.append("glyph %d" % cp)
        for row in font["glyphs"][cp]:
            lines.append("".join("#" if v else "." for v in row))
    write_text(path, "\n".join(lines) + "\n")


def write_widths(path, font, codepoints, note):
    lines = ["# advance widths in pixels for %s" % os.path.basename(path).replace(".widths", ".font"),
             "# %s" % note, "# <codepoint> <width>"]
    for cp in codepoints:
        lines.append("%d %d" % (cp, font["widths"].get(cp, 0)))
    write_text(path, "\n".join(lines) + "\n")


def font_sheet(font, codepoints, path, scale):
    if Image is None:
        return False
    cols = 16
    cw, ch = font["w"] * scale + 6, font["h"] * scale + 16
    rows = (len(codepoints) + cols - 1) // cols
    im = Image.new("RGB", (cols * cw + 4, rows * ch + 4), (40, 40, 40))
    d = ImageDraw.Draw(im)
    for n, cp in enumerate(codepoints):
        x0, y0 = 4 + (n % cols) * cw, 4 + (n // cols) * ch
        d.text((x0, y0), "%d %s" % (cp, chr(cp)), fill=(255, 255, 0))
        wv = font["widths"].get(cp, font["w"])
        for r, row in enumerate(font["glyphs"][cp]):
            for c, v in enumerate(row):
                col = (255, 255, 255) if v else ((0, 0, 0) if c < wv else (90, 0, 0))
                d.rectangle([x0 + c * scale, y0 + 11 + r * scale,
                             x0 + c * scale + scale - 1, y0 + 11 + r * scale + scale - 1], fill=col)
    im.save(path)
    return True


# --------------------------------------------------------------------------
# PCX title pictures
# --------------------------------------------------------------------------

def parse_pcx(path):
    b = open(path, "rb").read()
    if b[0] != 0x0A or b[2] != 1:
        raise ValueError("%s: not an RLE PCX" % path)
    bpp, planes, bpl = b[3], b[65], u16le(b, 66)
    xmin, ymin, xmax, ymax = (u16le(b, o) for o in (4, 6, 8, 10))
    w, h = xmax - xmin + 1, ymax - ymin + 1
    if bpp != 2 or planes != 1:
        raise ValueError("%s: only 2-bit single-plane (CGA) PCX supported, got %d bpp x %d planes" % (path, bpp, planes))
    need = bpl * h
    out = bytearray()
    i = 128
    while len(out) < need and i < len(b):
        c = b[i]
        i += 1
        if c >= 0xC0:
            out += bytes([b[i]]) * (c & 0x3F)
            i += 1
        else:
            out.append(c)
    if len(out) < need:
        raise ValueError("%s: truncated image data" % path)
    px = bytearray(w * h)
    for y in range(h):
        row = out[y * bpl:(y + 1) * bpl]
        for x in range(w):
            px[y * w + x] = (row[x // 4] >> (6 - 2 * (x % 4))) & 3
    return {"w": w, "h": h, "pixels": bytes(px), "consumed": i, "size": len(b)}


def write_img(path, img):
    hdr = b"EIMG" + img["w"].to_bytes(2, "little") + img["h"].to_bytes(2, "little")
    write_bytes(path, hdr + img["pixels"])


def write_img_png(path, img):
    if Image is None:
        return False
    im = Image.new("P", (img["w"], img["h"]))
    flat = []
    for rgb in CGA_RGB:
        flat += list(rgb)
    im.putpalette(flat + [0] * (768 - len(flat)))
    im.putdata(list(img["pixels"]))
    im.save(path)
    return True


# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------

MAP_NAMES = list("ABCDEFGHIJKL")
EXTRA_MAPS = ["CARIBBEN", "MED1", "SEASIA", "WEIRD1", "WEIRD2", "WEIRD3"]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("empdat", help="path to the original EMPDAT directory")
    ap.add_argument("--out", default=DEFAULT_OUT, help="output directory (default: assets/original)")
    args = ap.parse_args(argv)
    src, out = os.path.abspath(args.empdat), os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)
    os.makedirs(os.path.join(out, "maps"), exist_ok=True)
    if Image is None:
        print("warning: Pillow not available; PNG previews will be skipped", file=sys.stderr)
    names = icon_names()
    written = []
    problems = []

    def note(p):
        written.append(os.path.relpath(p, out))

    # ---- icons
    icon_sets = {}
    for kind, n in [("CGA", 1), ("CGA", 2), ("CGA", 3), ("CGA", 4), ("EGA", 1), ("EGA", 2)]:
        p = find_file(src, "EMPICON%d.%s" % (n, kind))
        if not p:
            problems.append("missing EMPICON%d.%s" % (n, kind))
            continue
        info = parse_icons(p)
        tag = "%s%d" % (kind.lower(), n)
        icon_sets[tag] = info
        tp = os.path.join(out, "tiles_%s.tiles" % tag)
        write_tiles(tp, info, names, n)
        note(tp)
        sx, sy = (4, 4) if info["kind"] == "cga" else (4, 8)
        sp = os.path.join(out, "tiles_%s_sheet.png" % tag)
        title = "%s  %d tiles %dx%d  shown at %dx%d  player colours %s" % (
            os.path.basename(p), info["count"], info["w"], info["h"], sx, sy,
            " ".join(map(str, info["colours"])))
        if icon_sheet(info, names, sp, sx, sy, title):
            note(sp)
        print("%-13s %d tiles %dx%d, player colour table %s" % (
            os.path.basename(p), info["count"], info["w"], info["h"], info["colours"]))

    print("\ncoast tile check (detected edge mask vs index-49):")
    for tag, info in icon_sets.items():
        res = coast_band_check(info)
        bad = [r for r in res if r[1] != r[2]]
        if not bad:
            print("  %s: band-diff rule confirms all 15 coast tiles" % tag)
            continue
        cc, res2 = coast_colour_check(info)
        bad2 = [r for r in res2 if r[1] != r[2]]
        print("  %s: band-diff mismatches %s; coast-colour(%d) rule mismatches %s" % (
            tag, [(t, e, d) for t, e, d in bad], cc, [(t, e, d) for t, e, d in bad2] or "none"))

    # ---- maps
    print("\nmaps:")
    maps = {}
    for nm in MAP_NAMES + EXTRA_MAPS:
        p = find_file(src, nm + ".MAP") or find_file(os.path.join(src, "XTRAMAPS"), nm + ".MAP")
        if not p:
            problems.append("missing %s.MAP" % nm)
            continue
        m = parse_map(p)
        maps[nm] = m
        ep = os.path.join(out, "maps", nm + ".emap")
        write_emap(ep, m, nm)
        note(ep)
        # checks
        rows = open(ep).read().splitlines()
        grid = [r for r in rows if not r.startswith("#")]
        assert len(grid) == ROWS and all(len(r) == COLS for r in grid), nm
        comp, ncity = landmass_city_counts(m["cells"])
        on_ring = [divmod(ci, COLS) for ci in m["cities"] if comp[ci] < 0]
        if on_ring:
            problems.append("%s: cities on the outer ring: %s" % (nm, on_ring))
        agree = sum(1 for ci, v in zip(m["cities"], m["city_vals"])
                    if comp[ci] >= 0 and ncity[comp[ci]] == v)
        masks = Counter(coast_mask(m["cells"], i) for i in range(MAPSIZE) if m["cells"][i] == 3)
        missing = [k for k in masks if coast_mask_name(k) not in names]
        if missing:
            problems.append("%s: coast masks without tile: %s" % (nm, missing))
        land = sum(1 for v in m["cells"] if v == 3)
        sea = sum(1 for v in m["cells"] if v == 2)
        print("  %-9s %4d B  %dx%d  cities %2d  land %4d sea %4d  ring[%s]  hdr %s (%s)  aux==landmass-cities %d/%d" % (
            nm, m["size"], len(grid[0]), len(grid), len(m["cities"]), land, sea,
            ring_summary(m["cells"]), m["header"].hex(), m["endian"], agree, len(m["cities"])))

    if "A" in maps and "cga1" in icon_sets:
        pp = os.path.join(out, "preview_A_map.png")
        if render_map(maps["A"], icon_sets["cga1"], names, pp):
            note(pp)
        if "ega1" in icon_sets:
            pe = os.path.join(out, "preview_A_map_ega1.png")
            if render_map(maps["A"], icon_sets["ega1"], names, pe):
                note(pe)
        m = maps["A"]
        masks = Counter(coast_mask(m["cells"], i) for i in range(MAPSIZE) if m["cells"][i] == 3)
        print("\nA.MAP land cells by coast mask: %s" % dict(sorted(masks.items())))

    # ---- fonts
    p = find_file(src, "CGAFNT.FY")
    if p:
        f = parse_fy(p)
        cps = sorted(f["glyphs"])
        fp = os.path.join(out, "font_cga.font")
        write_font(fp, f, cps)
        note(fp)
        wp = os.path.join(out, "font_cga.widths")
        write_widths(wp, f, cps, "columns of the 4-column cell that the glyph uses (left-aligned); "
                                 "space (32) is not stored in the file")
        note(wp)
        sp = os.path.join(out, "font_cga_sheet.png")
        if font_sheet(f, cps, sp, 6):
            note(sp)
        print("\nCGAFNT.FY: %d glyphs %d..%d, cell %dx%d" % (len(cps), cps[0], cps[-1], f["w"], f["h"]))
    else:
        problems.append("missing CGAFNT.FY")
    p = find_file(src, "GOTHIC1.ESI")
    if p:
        g = parse_esi(p)
        cps = [cp for cp in sorted(g["glyphs"]) if g["widths"].get(cp, 0) > 0]
        empty_but_wide = [cp for cp in cps if not any(any(r) for r in g["glyphs"][cp])]
        ink_zero_width = [cp for cp in sorted(g["glyphs"]) if g["widths"].get(cp, 0) == 0
                          and any(any(r) for r in g["glyphs"][cp])]
        overflow = [cp for cp in cps if any(row[c] for row in g["glyphs"][cp]
                                            for c in range(g["widths"][cp], g["w"]))]
        if empty_but_wide or ink_zero_width:
            problems.append("GOTHIC1.ESI glyph/width alignment suspicious: %s %s" % (empty_but_wide, ink_zero_width))
        fp = os.path.join(out, "font_gothic.font")
        write_font(fp, g, cps)
        note(fp)
        wp = os.path.join(out, "font_gothic.widths")
        write_widths(wp, g, [0x20] + cps, "from the width table in the file header; 32 (space) from the u16 after it")
        note(wp)
        sp = os.path.join(out, "font_gothic_sheet.png")
        if font_sheet(g, cps, sp, 2):
            note(sp)
        print("GOTHIC1.ESI: %d glyphs with ink, cell %dx%d, glyphs with ink past their width: %s" % (
            len(cps), g["w"], g["h"], [chr(c) for c in overflow] or "none"))
    else:
        problems.append("missing GOTHIC1.ESI")

    # ---- PCX
    for n in (1, 2):
        p = find_file(src, "EMPIRE%d.PCX" % n)
        if not p:
            problems.append("missing EMPIRE%d.PCX" % n)
            continue
        img = parse_pcx(p)
        ip = os.path.join(out, "title%d.img" % n)
        write_img(ip, img)
        note(ip)
        pp = os.path.join(out, "title%d.png" % n)
        if write_img_png(pp, img):
            note(pp)
        hist = Counter(img["pixels"])
        print("EMPIRE%d.PCX: %dx%d, consumed %d/%d bytes, colour histogram %s" % (
            n, img["w"], img["h"], img["consumed"], img["size"], dict(sorted(hist.items()))))

    print("\nwrote %d files under %s" % (len(written), out))
    if problems:
        print("PROBLEMS:")
        for pr in problems:
            print("  " + pr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
