# Interstel Empire v2.05 data formats

This file describes the data files of *Empire: Wargame of the Century* (Interstel,
Walter Bright and Mark Baldwin, DOS, v2.05E) as decoded by
`tools/interstel_import/import.py`, and the plain-text files that the importer
writes for the Rust game.

```
python3 tools/interstel_import/import.py reference/interstel/files_v205e/EMPDAT
# default output: assets/original/   (gitignored; the source files are commercial)
```

The importer uses only the standard library for the game-facing outputs
(`.tiles`, `.emap`, `.font`, `.widths`, `.img`). Pillow is needed only for the PNG
previews, which are skipped with a warning if it is missing. Re-running it
rewrites the same files with identical bytes.

## Summary and confidence

| File | Format | Confidence |
|---|---|---|
| `EMPICON1..4.CGA` | 12-byte header + 65 records of 8x8 px, 2 bpp | High: every byte accounted for; layout matches the EGA set slot for slot |
| `EMPICON1..2.EGA` | 12-byte header + 65 records of 16x8 px, 4 planes | High |
| Icon names | see table below | High for 0-34 and 37-64; medium for 35/36 (which cursor is which) |
| `*.MAP` terrain | 4-byte header, backwards RLE, 0 terminator | High: all 18 maps fill exactly 6000 cells; A/B/C match Walter Bright's own canned maps cell for cell (98-99%) |
| `*.MAP` city records | one big-endian u16 per city | High for layout; low for meaning |
| `CGAFNT.FY` | XOR obfuscation + 0x55 RLE + 4x8 column glyphs + width table | High |
| `GOTHIC1.ESI` | clear header + widths, XOR-obfuscated 24x24 glyphs | High for glyph bitmaps and widths; header bytes partly unknown |
| `EMPIRE1/2.PCX` | standard PCX v2, 320x200, 2 bpp | High |

Multi-byte integers are little-endian unless stated otherwise.

## Icon sets: EMPICONn.CGA and EMPICONn.EGA

### Layout

```
offset  size  field
0       u16   record count            = 65 (0x41)
2       u16   bytes per record        = 20 (CGA) / 68 (EGA)
4       u16   tile width in pixels    = 8 (CGA) / 16 (EGA)
6       u16   tile height in pixels   = 8
8       u8[4] colour per player slot 0..3 (see below)
12      65 records, back to back
```

File size check: CGA 12 + 65*20 = 1312, EGA 12 + 65*68 = 4432. So there are 65
tiles, not 82 (1312/16 would be 82, but each record has a 4-byte prefix and the
file has a 12-byte header).

Each record:

```
u16  bits per row per plane = 16    (CGA: 8 px * 2 bpp; EGA: 16 px * 1 bit)
u16  rows                   = 8
bitmap
```

* **CGA bitmap** (16 bytes): 8 rows of 2 bytes, 2 bits per pixel, packed exactly
  like CGA video memory: the most significant bit pair of each byte is the
  leftmost pixel. Pixel values are CGA palette 1 indices: 0 black, 1 cyan,
  2 magenta, 3 white.
* **EGA bitmap** (64 bytes): 8 rows; each row is 4 planes of 2 bytes (plane 0,
  1, 2, 3), each plane big-endian with bit 15 = leftmost pixel. Pixel value =
  p0*1 + p1*2 + p2*4 + p3*8, i.e. the standard IBM EGA colour (plane 0 blue,
  1 green, 2 red, 3 intensity). Sea comes out as 9 (light blue) and land as
  2 (green), which matches the manual's colour table. The tiles are 16x8 because
  the EGA mode is 640x200, where a 16x8 tile is about as square on screen as an
  8x8 tile in the 320x200 CGA mode.

### Player colour table (header bytes 8-11)

| File | slot 0 | slot 1 | slot 2 | slot 3 |
|---|---|---|---|---|
| all four CGA sets | 0 | 3 white | 2 magenta | 1 cyan |
| EMPICON1.EGA | 0 | 15 white | 12 light red | 14 yellow |
| EMPICON2.EGA | 0 | 12 light red | 13 light magenta | 14 yellow |

Slots 1-3 are players 1-3. This matches the manual's Table I (CGA: player 1
white, 2 magenta, 3 cyan; EGA: white, red, yellow) and README.DOC ("In icon set 2,
player 1 is red and player 2 is magenta"). The per-player unit icons are stored
already coloured, so the table is redundant with the pixels; the game probably
uses it for text or report colours. Slot 0 is always 0.

### The four CGA sets and two EGA sets

The 65 slots have the same meaning in every file. Only the terrain art differs:

* CGA set 1: sea solid cyan with white wave flecks; land a black/cyan
  checkerboard; coast lines white.
* CGA set 2: white land, magenta coast lines.
* CGA set 3: magenta land; the coast is drawn as cyan (sea-coloured) notches on
  the outer pixels, and the land-fighter frame is magenta.
* CGA set 4: like set 1 with magenta coast lines.
* EGA sets 1 and 2 differ only in the player colours above.

EGA tile 4 (`city_p1`) has a few stray coloured pixels in its right-hand column
in both EGA files. That is in the data, not a decoding error.

### Icon index table (EMPICON1.CGA; same slots in every set)

`_p1/_p2/_p3` are players 1-3 (CGA white/magenta/cyan). Within each player block
(4-13, 14-23, 24-33) the unit order is the classic Empire order A F D T S R C B,
with the fighter stored twice.

| # | name | description |
|---|---|---|
| 0 | `unknown` | unexplored square (solid black) |
| 1 | `city_neutral` | neutral (unowned) city: black grid on white, the exact pixel inverse of tile 4 (manual: "Neutral Cities: Black on White") |
| 2 | `sea` | open sea |
| 3 | `land` | land with no orthogonal sea neighbour (coast mask 0) |
| 4 | `city_p1` | owned city (grid of player-colour lines on black), player 1 |
| 5 | `army_p1` | army (tank silhouette), player 1 |
| 6 | `fighter_p1` | fighter over land (CGA: dotted land-texture L along left/bottom edge; EGA: green background), player 1 |
| 7 | `fighter_sea_p1` | fighter over sea (CGA: solid sea-colour L along left/bottom edge; EGA: blue background), player 1 |
| 8 | `destroyer_p1` | destroyer (smallest hull), player 1 |
| 9 | `transport_p1` | troop transport, empty (EGA: smoke puffs), player 1 |
| 10 | `submarine_p1` | submarine (flat hull with conning tower), player 1 |
| 11 | `cruiser_p1` | cruiser, player 1 |
| 12 | `carrier_p1` | aircraft carrier, empty deck, player 1 |
| 13 | `battleship_p1` | battleship (largest, most turrets), player 1 |
| 14 | `city_p2` | owned city (grid of player-colour lines on black), player 2 (CGA magenta) |
| 15 | `army_p2` | army (tank silhouette), player 2 (CGA magenta) |
| 16 | `fighter_p2` | fighter over land (CGA: dotted land-texture L along left/bottom edge; EGA: green background), player 2 (CGA magenta) |
| 17 | `fighter_sea_p2` | fighter over sea (CGA: solid sea-colour L along left/bottom edge; EGA: blue background), player 2 (CGA magenta) |
| 18 | `destroyer_p2` | destroyer (smallest hull), player 2 (CGA magenta) |
| 19 | `transport_p2` | troop transport, empty (EGA: smoke puffs), player 2 (CGA magenta) |
| 20 | `submarine_p2` | submarine (flat hull with conning tower), player 2 (CGA magenta) |
| 21 | `cruiser_p2` | cruiser, player 2 (CGA magenta) |
| 22 | `carrier_p2` | aircraft carrier, empty deck, player 2 (CGA magenta) |
| 23 | `battleship_p2` | battleship (largest, most turrets), player 2 (CGA magenta) |
| 24 | `city_p3` | owned city (grid of player-colour lines on black), player 3 (CGA cyan) |
| 25 | `army_p3` | army (tank silhouette), player 3 (CGA cyan) |
| 26 | `fighter_p3` | fighter over land (CGA: dotted land-texture L along left/bottom edge; EGA: green background), player 3 (CGA cyan) |
| 27 | `fighter_sea_p3` | fighter over sea (CGA: solid sea-colour L along left/bottom edge; EGA: blue background), player 3 (CGA cyan) |
| 28 | `destroyer_p3` | destroyer (smallest hull), player 3 (CGA cyan) |
| 29 | `transport_p3` | troop transport, empty (EGA: smoke puffs), player 3 (CGA cyan) |
| 30 | `submarine_p3` | submarine (flat hull with conning tower), player 3 (CGA cyan) |
| 31 | `cruiser_p3` | cruiser, player 3 (CGA cyan) |
| 32 | `carrier_p3` | aircraft carrier, empty deck, player 3 (CGA cyan) |
| 33 | `battleship_p3` | battleship (largest, most turrets), player 3 (CGA cyan) |
| 34 | `border` | inaccessible map edge: fine black/white checkerboard (reads as grey; neither palette has a real grey here) |
| 35 | `cursor_direction` | probably the Go Direction cursor: four corner blocks around a centre dot, like the eight squares around a unit |
| 36 | `cursor_survey` | probably the survey-mode cursor: corner brackets with a hollow centre |
| 37 | `explosion` | combat explosion (white/magenta/cyan in CGA, red/yellow in EGA) |
| 38 | `city_occupied_p1` | owned city with units inside: the tile 4 grid with corners filled, denser ("Cities with units in them will be shaded") |
| 39 | `city_occupied_p2` | same, player 2 |
| 40 | `city_occupied_p3` | same, player 3 |
| 41 | `army_sentry_p1` | army on sentry duty (tent; EGA draws it on green land) |
| 42 | `army_sentry_p2` | same, player 2 |
| 43 | `army_sentry_p3` | same, player 3 |
| 44 | `transport_loaded_p1` | troop transport with armies aboard: tile 9 with a row of portholes |
| 45 | `transport_loaded_p2` | same, player 2 |
| 46 | `transport_loaded_p3` | same, player 3 |
| 47 | `carrier_loaded_p1` | aircraft carrier with fighters on deck: tile 12 with planes on the deck row |
| 48 | `carrier_loaded_p2` | same, player 2 |
| 49 | `carrier_loaded_p3` | same, player 3 |
| 50 | `land_coast_1` | land, coast line on the N edge |
| 51 | `land_coast_2` | land, coast on E |
| 52 | `land_coast_3` | land, coast on N+E |
| 53 | `land_coast_4` | land, coast on S |
| 54 | `land_coast_5` | land, coast on N+S |
| 55 | `land_coast_6` | land, coast on E+S |
| 56 | `land_coast_7` | land, coast on N+E+S |
| 57 | `land_coast_8` | land, coast on W |
| 58 | `land_coast_9` | land, coast on N+W |
| 59 | `land_coast_10` | land, coast on E+W |
| 60 | `land_coast_11` | land, coast on N+E+W |
| 61 | `land_coast_12` | land, coast on S+W |
| 62 | `land_coast_13` | land, coast on N+S+W |
| 63 | `land_coast_14` | land, coast on E+S+W |
| 64 | `land_coast_15` | land, coast on all four edges |

How the names were assigned:

* The EGA sheet resolves most questions because EGA draws terrain behind units:
  tile 6 has a green (land) background and tile 7 a blue (sea) one; tiles 41-43
  are tents on green; tiles 44-49 show portholes and planes.
* Ships: tile 10 is the only flat, low hull with a tower (submarine), tile 12 the
  long flat deck (carrier), tile 13 the tallest (battleship), tile 8 the smallest
  (destroyer), tile 9 the bulky hull with smoke that also appears loaded in 44
  (transport). The remaining one, 11, is the cruiser. The resulting order 5-13
  = A F F D T S R C B is the unit order used throughout Bright's Empire sources.
* Cursors 35 and 36: there are only two cursor-like tiles and three cursor
  modes in the manual (blinking move cursor, survey cursor, direction cursor).
  The move-mode cursor is described as "blinking" over the unit and probably
  does not need its own tile. Which of 35/36 is survey and which is direction
  is a guess from their shapes; it has not been checked in EMPIRE.EXE (see Open questions).

### Coast mask rule

`mask = N*1 + E*2 + S*4 + W*8`, where each bit is set when the orthogonal
neighbour of a land square is sea. Tile index = 49 + mask for mask 1..15, and
tile 3 for mask 0. Diagonal-only sea neighbours do not change the tile (there
are no corner variants).

Verification (run by the importer every time):

* For each coast tile, compare the middle half of a 2-pixel band along each edge
  (4 px wide on EGA's vertical edges) against plain land. Edges with >20% of
  pixels changed are taken as coast. This gives exactly mask = index - 49 for
  CGA sets 1, 2, 4 and both EGA sets.
* CGA set 3 shifts its land texture between tiles, so the band test is noisy.
  Counting cyan (sea-colour) pixels gained on each outer line instead gives
  mask = index - 49 for 14 of 15 tiles; tile 50 (N) gains only 2 cyan pixels on
  its top edge, which is below the test's threshold but still the only edge
  that gains any.
* Every land square in all 18 maps has a mask with an existing tile (trivially
  true, since all 16 masks exist). In A.MAP the land squares by mask are
  {0: 523, 1: 153, 2: 100, 3: 67, 4: 139, 5: 60, 6: 73, 7: 57, 8: 94, 9: 69,
  10: 21, 11: 20, 12: 80, 13: 51, 14: 22, 15: 18}.
* `preview_A_map.png` renders A.MAP with these rules: border tile on the outer
  ring, neutral city tile for cities. The coast lines sit on the sea-facing edges.

Not determined: whether the original treats unexplored or border squares as sea
when it picks a coast tile. The preview uses the stored terrain: the ring is
sea in A-J, so land next to it gets a coast line there.

## Maps: *.MAP

```
offset  size   field
0       4      header: row max 59 and col max 99 as two u16
               (BIG-endian 00 3B 00 63 in A-J and all XTRAMAPS;
                little-endian 3B 00 63 00 in K and L)
4       n      terrain RLE stream, terminated by a 0x00 byte
4+n+1   2*C    C city records, one big-endian u16 each (C = number of city cells)
```

There is nothing after the city records; every file is fully accounted for.

**Terrain RLE.** Each byte `c` is a run: terrain = `c & 3` (1 city, 2 sea,
3 land; 0 is invalid), length = `((c >> 2) & 63) + 1`. Runs are written starting
at cell index 5999 and going *backwards* to 0, where cell index = row*100 + col
and row 0 is the top. A 0x00 byte ends the stream. This is the scheme in Walter
Bright's D source (`init.d`, `selmap`). The terrain codes are also the D version's `map[]` codes.

Evidence for orientation: the canned maps in `Empire-D/mapdata.d` decode with
the same routine to near-copies of the Interstel maps: D `a.map` matches A.MAP in
5892/6000 cells, `b.map` matches B.MAP in 5859, `c.map` matches C.MAP in 5871.
The 180-degree rotation matches much worse (3449-3736 cells). D's
`ROW(loc) = loc / 100` confirms row-major order with row 0 at the top. Also, the
city records only line up with the landmass statistics (below) when cities are
taken in ascending cell order.

**City records.** After the terminator there is exactly one big-endian u16 per
city cell, in ascending cell order (row-major from the top-left). The values are
small (1-9 in A-L and WEIRD2, up to 26 in CARIBBEN and MED1). Their meaning is
not confirmed:

* In most standard maps the value equals the number of cities on the city's
  8-connected landmass (ignoring the outer ring). That matches 757 of the 787
  cities in A-L. It is exact for D, H, L and WEIRD2 and off for only a few
  cities in the others.
* It fails on big continents (CARIBBEN 12/62, MED1 9/80, WEIRD1 0/67): the
  value is smaller than the continent's city count.
* The best fit I found counts cities on the same landmass whose Chebyshev
  distance is at most 24 (a 49x49 window). That is exact for A, D, H, L, SEASIA
  and WEIRD2 and wrong for 88 of all 1200 cities (B 3, C 2, E 2, F 5, G 3, I 2,
  J 2, K 5, CARIBBEN 13, MED1 14, WEIRD1 24, WEIRD3 13). Other variants were
  worse: other window sizes, Euclidean or Manhattan distance, a flood fill
  clipped to the window, land-only paths, and 4-connectivity.
* The manual says the editor must "process the city's information" before
  saving, so this is derived data that the editor writes on save, probably for
  the computer player. It may also be stale where a map was edited after its
  last processing.

The importer passes the value through untouched as `# city <row> <col> <aux>`.
The Rust game can recompute whatever it needs from the terrain.

**Outer ring.** Every map stores the ring (row 0, row 59, col 0, col 99; 316
cells) as all sea, except K and L, which store it as all land. No map has a
city on the ring. The game treats the ring as the inaccessible border whatever
is stored there. K and L also have the little-endian header, so they were
probably written by a different editor build.

### Per-map results (all checks run by the importer)

| map | bytes | size | cities | land | sea | ring | header | aux = landmass city count |
|---|---|---|---|---|---|---|---|---|
| A | 1018 | 100x60 | 64 | 1547 | 4389 | sea | BE | 62/64 |
| B | 1064 | 100x60 | 66 | 1577 | 4357 | sea | BE | 59/66 |
| C | 961 | 100x60 | 69 | 1468 | 4463 | sea | BE | 67/69 |
| D | 1082 | 100x60 | 72 | 1178 | 4750 | sea | BE | 72/72 |
| E | 1085 | 100x60 | 65 | 1384 | 4551 | sea | BE | 63/65 |
| F | 1145 | 100x60 | 69 | 1275 | 4656 | sea | BE | 64/69 |
| G | 1141 | 100x60 | 71 | 1566 | 4363 | sea | BE | 68/71 |
| H | 1135 | 100x60 | 61 | 1473 | 4466 | sea | BE | 61/61 |
| I | 1173 | 100x60 | 66 | 1391 | 4543 | sea | BE | 64/66 |
| J | 1025 | 100x60 | 62 | 1383 | 4555 | sea | BE | 60/62 |
| K | 989 | 100x60 | 60 | 1239 | 4701 | **land** | **LE** | 55/60 |
| L | 1045 | 100x60 | 62 | 1557 | 4381 | **land** | **LE** | 62/62 |
| CARIBBEN | 631 | 100x60 | 62 | 1799 | 4139 | sea | BE | 12/62 |
| MED1 | 722 | 100x60 | 80 | 3509 | 2411 | sea | BE | 9/80 |
| SEASIA | 788 | 100x60 | 80 | 1399 | 4521 | sea | BE | 61/80 |
| WEIRD1 | 1741 | 100x60 | 67 | 2643 | 3290 | sea | BE | 0/67 |
| WEIRD2 | 453 | 100x60 | 71 | 272 | 5657 | sea | BE | 71/71 |
| WEIRD3 | 942 | 100x60 | 53 | 3113 | 2834 | sea | BE | 22/53 |

The A-L city counts (60-72) agree with the manual's "60 to 70 cities". MED1 and
SEASIA have 80.

## Fonts

Both font files are XOR-obfuscated with the 14-byte string `(C)1984WKMASON`,
with bit 7 set on every byte (`A8 C3 A9 B1 B9 B8 B4 D7 CB CD C1 D3 CF CE`). The
string is visible in GOTHIC1.ESI, where runs of zero bytes encrypt to the bare
key.

### CGAFNT.FY (474 bytes): the small CGA font

1. XOR the whole file with the key, key index = file offset mod 14.
2. Expand the RLE: byte 0x55 is an escape, and `55 <count> <value>` means
   `<count>` copies of `<value>`. All other bytes are literal. The result is
   478 bytes.
3. Decoded layout:

```
0     u8[8]   header: 04 00 04 01 5E 01 01 08
              byte 0 = 4 columns per glyph cell, byte 4 = 94 glyphs, byte 7 = 8 rows;
              bytes 1-3, 5, 6 unknown
8     94 * 4  glyphs for codepoints 33..126 ('!'..'~'), 4 bytes each.
              Each byte is one COLUMN, left to right; bit 7 is the top row.
392   u8[94]  width table: columns actually used by each glyph (1..4),
              glyphs left-aligned in the cell
```

It is a 3x5 capital-letter font (rows 2-6 of the 8-row cell) for the 320x200
mode. Lower case 'a'-'z' are copies of 'A'-'Z'. Space (32) is not stored.
Several cells hold uninitialised memory, not glyphs: ';' (59) is `00 00 8B 5E`,
'`' (96) is `FE 36 8B 47` and '{' (123) is `3B 06 22 F5` (the 8B xx bytes are
x86 instructions). '"' (34) is `01 20 20 20`, which does not look like a quote
mark either. Some cells look deliberate but are not their ASCII character: '$'
(36) is a solid 4x8 block, '&' (38) a 4-wide overline, '@' (64) a small diamond,
'}' (125) and '~' (126) small symbols. They are probably UI symbols placed on
unused codepoints. Width table, examples: '.' 1, '1' 2, '[' 2, digits and
letters 3, '_' 4.

Confidence: high. Digits, both alphabets and the width table all line up
exactly at a fixed 4-byte stride after RLE expansion.

### GOTHIC1.ESI (6949 bytes): the gothic title font

```
0     u8[12]   header, not obfuscated: 12 5F 01 01 14 13 03 00 48 00 0A 03
               byte 1 = 95 glyphs, u16@6 = 3 bytes per row, u16@8 = 72 bytes per glyph
               (so 24 rows of 24 px); the rest unknown (0x12 = 18 may be the
               cap height, 0x14/0x13 = 20/19 baseline or line height)
12    u8[95]   advance widths in pixels for codepoints 32..126 (not obfuscated);
               0 for ' ', '@', '['..'`' and '{'..'~', which have no glyph
107   u16      10, probably the width of a space
109   95 * 72  glyph bitmaps for codepoints 33..127, XOR-obfuscated with the key
               starting at key index 0 at offset 109. Each glyph is 24 rows x
               3 bytes, row-major, MSB = leftmost pixel.
```

The glyph-to-width alignment is checked: every zero-width code has an empty
bitmap, every non-zero-width code has ink, and no glyph has ink past its width.
83 glyphs carry ink: `! " # $ % & ' ( ) * + , - . /`, digits, `: ; < = > ?`,
A-Z, a-z.

## Title pictures: EMPIRE1.PCX, EMPIRE2.PCX

Standard ZSoft PCX version 2: RLE (a byte >= 0xC0 gives a count `b & 0x3F`
followed by the value byte), 2 bits per pixel, 1 plane, 80 bytes per line,
window 0,0-319,199. MSB pair = leftmost pixel, as in CGA memory. The 48-byte
header palette and the filler after byte 70 hold uninitialised memory (x86 code
and the font names `WR18FNT`, `HERALD18FNT`), so the palette is not taken from
the file. CGA palette 1 high intensity (black/cyan/magenta/white) gives exactly
the colours in the MobyGames CGA screenshots. Both files decode to 16000 bytes
using every byte of the file.

* EMPIRE1.PCX: the Interstel Corporation logo screen.
* EMPIRE2.PCX: the EMPIRE title picture (general, landing craft, soldiers).

## Output formats written by the importer

All text files use `\n` line endings. Files in `assets/original/`:

| file | contents |
|---|---|
| `tiles_cga1..4.tiles` | 65 tiles each, `palette cga1`, 8x8 |
| `tiles_ega1..2.tiles` | 65 tiles each, `palette ega`, written as `tile <name> 16 8` with hex digits 0-f |
| `tiles_*_sheet.png` | contact sheets with index and name labels: CGA at 4x, EGA at 4x horizontally and 8x vertically (aspect-corrected) |
| `maps/<NAME>.emap` | 18 maps: A-L, CARIBBEN, MED1, SEASIA, WEIRD1-3 |
| `font_cga.font`, `font_cga.widths` | 4x8 cells, 94 glyphs (33-126) |
| `font_gothic.font`, `font_gothic.widths` | 24x24 cells, the 83 glyphs that have ink |
| `font_*_sheet.png` | glyph previews (black = inside the advance width, dark red = outside) |
| `title1.img/.png`, `title2.img/.png` | EMPIRE1.PCX / EMPIRE2.PCX |
| `preview_A_map.png`, `preview_A_map_ega1.png` | A.MAP rendered with icon set CGA 1 / EGA 1 |

**`.tiles`**: header comments, then `palette cga1|ega`, then for every tile in
file order `# index NN`, `tile <name>` (or `tile <name> <w> <h>` when not 8x8)
and h lines of w palette digits. A blank line follows each tile. The comments
also record the header colour table and the coast-mask convention.

**`.emap`**: comment lines first, then exactly 60 lines of 100 characters
(`.` sea, `+` land, `*` city). The comments include the header bytes, the ring
contents and one `# city <row> <col> <aux>` line per city in ascending cell
order, where `aux` is the raw u16 described above. The outer ring is written
as stored (sea in most maps, land in K and L).

**`.font`**: exactly as specified, with no comment lines: `font <w> <h>`, then
`glyph <codepoint>` followed by h lines of w characters (`#` ink, `.` blank).
Glyphs are the full stored cells, left-aligned.

**`.widths`** (sidecar, so the `.font` format stays unchanged): `#` comment
lines, then `<codepoint> <advance width in pixels>` per line. For the CGA font
this is the column count from the file's width table; the original probably
adds a 1-column gap between glyphs (digits use 3 of 4 columns). Space is not in
the file. For the gothic font, 32 (space) is listed with the u16 after the width
table (10).

**`.img`**: `"EIMG"`, u16 LE width, u16 LE height, then width*height bytes, one
CGA palette index (0-3) per pixel, row-major from the top-left. The files are
8 + 320*200 = 64008 bytes.

## Open questions

* **City u16 meaning.** See above. The best rule found is wrong for 7% of
  cities. The REA disassembler available when this was written could not load
  16-bit DOS executables (it needs a Ghidra install), so nobody has traced the editor's save
  routine in EMPIRE.EXE. The strings "Error in reading map!" and ".MAP file
  write error!" in the EXE mark where to start.
* **Cursor tiles 35/36.** Which one is the survey cursor and which the direction
  cursor is a guess from their shapes.
* **Header bytes.** Icon header slot 0 (always 0), FY header bytes 1-3/5/6 and
  most of the ESI header are not decoded. Nothing in the decoding depends on
  them.
* **Coast lines next to unexplored or border squares** (see the coast section).
