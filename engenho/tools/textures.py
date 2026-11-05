"""Draws Engenho's textures (16x16 pixel art, original) into the pack.

    python3 engenho/tools/textures.py

Each item is ASCII art with a palette; blocks are built from a metal casing
panel with a part drawn on it. Run again after a change: the PNGs are
written next to the pack's config.
"""
import os
import struct
import sys
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "packs", "engenho",
                    "resources", "assets", "engenho", "textures")


def png(pixels):
    raw = b"".join(b"\x00" + b"".join(bytes(p) for p in row) for row in pixels)

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 16, 16, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def save(path, pixels):
    full = os.path.join(ROOT, path + ".png")
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "wb") as f:
        f.write(png(pixels))


def blank():
    return [[(0, 0, 0, 0) for _ in range(16)] for _ in range(16)]


def art(rows, palette):
    rows = [r for r in rows.strip("\n").split("\n")]
    assert len(rows) == 16 and all(len(r) == 16 for r in rows), [len(r) for r in rows]
    return [[palette.get(c, (0, 0, 0, 0)) for c in r] for r in rows]


def shades(dark, mid, light, shine):
    return {"K": (24, 24, 30, 255), "D": dark + (255,), "M": mid + (255,), "L": light + (255,),
            "W": shine + (255,)}


COBALT = shades((22, 46, 120), (40, 86, 190), (88, 140, 235), (190, 215, 255))
IRON = shades((110, 110, 116), (170, 170, 178), (210, 210, 216), (245, 245, 250))
GOLD = shades((160, 110, 20), (225, 175, 40), (250, 215, 80), (255, 245, 180))
COPPER = shades((130, 60, 30), (195, 105, 60), (230, 145, 95), (255, 205, 170))

INGOT = """
................
................
................
................
.....KKKKKKK....
....KWLLLLLMK...
...KLLLLLLMMK...
..KLLMMMMMMDK...
..KMMMMMMMDDK...
..KMMMMMMDDK....
..KDDDDDDDK.....
...KKKKKKK......
................
................
................
................
"""
DUST = """
................
................
................
................
................
.......L........
......LWM.......
.....MLLMM......
....MLMMLMD.....
...MMLMMMMMD....
..MLMMDMMMDMD...
..DMMMMDMMMMDD..
...DDDDDDDDDD...
................
................
................
"""
PLATE = """
................
................
..KKKKKKKKKKKK..
..KWLLLLLLLLMK..
..KLLLLLLLLLMK..
..KLLMMMMMMLMK..
..KLLMMMMMMLMK..
..KLLMMMMMMLMK..
..KLLMMMMMMLMK..
..KLLMMMMMMLMK..
..KLLMMMMMMLMK..
..KLLLLLLLLLMK..
..KMMMMMMMMMDK..
..KKKKKKKKKKKK..
................
................
"""
RAW = """
................
................
................
.....KKKK.......
....KLLWLKK.....
...KLMMLLLMK....
...KMMDMMLMMK...
..KLMDDMMMMMK...
..KMMMMMDMMDK...
..KDMMMMMMDDK...
...KDMMDMMDK....
....KDDDDDK.....
.....KKKKK......
................
................
................
"""
CIRCUIT = """
................
................
..KKKKKKKKKKKK..
..KGGGGGGGGGGK..
..KGYYGGGGYYGK..
..KGGYGCCGYGGK..
..KGGYYCCYYGGK..
..KGGGGCCGGGGK..
..KGYYGCCGRRGK..
..KGGYGGGGRRGK..
..KGGYYYYYGGGK..
..KGGGGGGGGGGK..
..KKKKKKKKKKKK..
...Y.Y.Y.Y.Y....
................
................
"""
CASING = """
................
..KKKKKKKKKKKK..
..KLLLLLLLLLMK..
..KLWMMMMMMWMK..
..KLMDDDDDDMMK..
..KLMDKKKKDMMK..
..KLMDKBBKDMMK..
..KLMDKBBKDMMK..
..KLMDKKKKDMMK..
..KLMDDDDDDMMK..
..KLWMMMMMMWMK..
..KMMMMMMMMMDK..
..KKKKKKKKKKKK..
................
................
................
"""
CHIP = """
................
................
...KKKKKKKKKK...
..KAAAAAAAAAAK..
..KAMMMMMMMMAK..
..KAMXXXXXXMAK..
..KAMXSSSSXMAK..
..KAMXSSSSXMAK..
..KAMXSSSSXMAK..
..KAMXXXXXXMAK..
..KAMMMMMMMMAK..
..KAAAAAAAAAAK..
...KKKKKKKKKK...
...Y.Y.Y.Y.Y....
................
................
"""


def items():
    for name, metal in [("cobalto", COBALT), ("ferro", IRON), ("ouro", GOLD), ("cobre", COPPER)]:
        save(f"item/po_{name}", art(DUST, metal))
        if name != "ouro":
            save(f"item/placa_{name}", art(PLATE, metal))
    save("item/lingote_cobalto", art(INGOT, COBALT))
    save("item/cobalto_bruto", art(RAW, COBALT))
    save("item/circuito", art(CIRCUIT, {"K": (20, 30, 20, 255), "G": (30, 110, 60, 255),
                                         "Y": (225, 190, 70, 255), "C": (40, 40, 48, 255),
                                         "R": (200, 40, 40, 255)}))
    save("item/carcaca", art(CASING, dict(IRON, B=(40, 86, 190, 255))))
    chip = {"K": (24, 24, 30, 255), "M": (60, 60, 70, 255), "X": (30, 30, 36, 255),
            "Y": (225, 190, 70, 255)}
    save("item/upgrade_velocidade", art(CHIP, dict(chip, A=(70, 170, 230, 255), S=(140, 220, 255, 255))))
    save("item/upgrade_eficiencia", art(CHIP, dict(chip, A=(60, 180, 80, 255), S=(150, 240, 150, 255))))


def noise(x, y, seed):
    """A small, repeatable variation per pixel."""
    h = (x * 374761393 + y * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    h = (h ^ (h >> 13)) * 1274126177 & 0xFFFFFFFF
    return ((h >> 16) & 0xFF) / 255.0


def metal_panel(seed, base=(120, 124, 132)):
    """A casing face: brushed metal, a dark frame, rivets in the corners."""
    p = blank()
    for y in range(16):
        for x in range(16):
            v = int(noise(x, y, seed) * 14) - 7
            c = tuple(max(0, min(255, b + v)) for b in base)
            p[y][x] = c + (255,)
    for i in range(16):
        for (x, y) in [(i, 0), (i, 15), (0, i), (15, i)]:
            p[y][x] = (52, 54, 60, 255)
    for i in range(1, 15):
        p[1][i] = (160, 164, 172, 255)
        p[i][1] = (150, 154, 162, 255)
        p[14][i] = (88, 90, 98, 255)
        p[i][14] = (92, 94, 102, 255)
    for (x, y) in [(2, 2), (13, 2), (2, 13), (13, 13)]:
        p[y][x] = (200, 204, 210, 255)
        p[y + 1 if y < 13 else y - 1][x] = (70, 72, 80, 255)
    return p


def rect(p, x0, y0, x1, y1, c):
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            p[y][x] = c + (255,) if len(c) == 3 else c


def stripe(p, y, c):
    for x in range(1, 15):
        p[y][x] = c + (255,)


BLUE = (40, 86, 190)
BLUE_LIGHT = (88, 140, 235)
DARK = (30, 32, 38)


def blocks():
    side = metal_panel(1)
    stripe(side, 7, BLUE)
    stripe(side, 8, BLUE_LIGHT)
    save("block/carcaca_lado", side)
    top = metal_panel(2)
    rect(top, 5, 5, 10, 10, DARK)
    rect(top, 6, 6, 9, 9, BLUE)
    rect(top, 7, 7, 8, 8, BLUE_LIGHT)
    save("block/carcaca_topo", top)
    save("block/carcaca_base", metal_panel(3, (96, 98, 106)))

    # Grinder: two toothed rollers in a dark mouth.
    for on in (False, True):
        f = metal_panel(4)
        rect(f, 3, 4, 12, 11, DARK)
        teeth = (230, 120, 40) if on else (150, 152, 160)
        for x in range(3, 13):
            f[5 + (x % 2)][x] = teeth + (255,)
            f[10 - (x % 2)][x] = teeth + (255,)
        rect(f, 3, 7, 12, 8, (70, 72, 80))
        if on:
            for x in range(4, 12, 2):
                f[7][x] = (255, 200, 90, 255)
        stripe(f, 13, BLUE)
        save("block/moedor_frente" + ("_on" if on else ""), f)

    # Electric furnace: a window with a coil, red-hot when on.
    for on in (False, True):
        f = metal_panel(5)
        rect(f, 3, 3, 12, 12, DARK)
        rect(f, 4, 4, 11, 11, (60, 30, 20) if on else (40, 42, 48))
        coil = (255, 120, 40) if on else (110, 70, 60)
        for y in (6, 9):
            for x in range(4, 12):
                f[y][x] = coil + (255,)
        for y in range(6, 10):
            f[y][4 if y < 8 else 11] = coil + (255,)
        if on:
            for x in range(5, 11, 2):
                f[7][x] = (255, 200, 80, 255)
        save("block/forno_frente" + ("_on" if on else ""), f)

    # Press: a ram over an anvil plate.
    for on in (False, True):
        f = metal_panel(6)
        rect(f, 3, 3, 12, 12, DARK)
        ram_y = 7 if on else 5
        rect(f, 6, 3, 9, ram_y, (150, 152, 160))
        rect(f, 4, ram_y + 1, 11, ram_y + 2, (190, 192, 200))
        rect(f, 4, 11, 11, 12, BLUE)
        if on:
            f[10][5] = f[10][10] = (255, 230, 120, 255)
        save("block/prensa_frente" + ("_on" if on else ""), f)

    # Generator: a firebox with a grille.
    for on in (False, True):
        f = metal_panel(7)
        rect(f, 3, 6, 12, 12, DARK)
        if on:
            for y in range(8, 12):
                for x in range(4, 12):
                    v = noise(x, y, 9)
                    f[y][x] = ((255, 190, 60, 255) if v > 0.6 else (230, 90, 30, 255)
                               if v > 0.25 else (150, 40, 20, 255))
        for x in range(3, 13, 2):
            for y in range(6, 13):
                f[y][x] = (90, 92, 100, 255)
        rect(f, 4, 2, 11, 4, (70, 72, 80))
        rect(f, 5, 3, 10, 3, (255, 90, 60) if on else (90, 40, 40))
        save("block/gerador_frente" + ("_on" if on else ""), f)

    # Solar panel: blue cells in a grid.
    s = blank()
    for y in range(16):
        for x in range(16):
            edge = x in (0, 15) or y in (0, 15)
            grid = x % 5 == 0 or y % 5 == 0
            v = int(noise(x, y, 11) * 20)
            s[y][x] = ((180, 182, 190, 255) if edge else (200, 205, 215, 255) if grid
                       else (20 + v, 40 + v, 110 + v, 255))
    for (x, y) in [(2, 2), (7, 2), (12, 2), (2, 7), (7, 7), (12, 7), (2, 12), (7, 12), (12, 12)]:
        s[y][x] = (120, 170, 255, 255)
    save("block/solar_topo", s)

    # Battery: a gauge on the front, terminals on the sides.
    f = metal_panel(8)
    rect(f, 5, 3, 10, 12, DARK)
    for i, y in enumerate(range(11, 3, -1)):
        f[y][6] = f[y][7] = f[y][8] = f[y][9] = ((60, 200, 90, 255) if i < 5
                                                  else (40, 60, 45, 255))
    rect(f, 6, 13, 9, 13, BLUE)
    save("block/bateria_frente", f)
    b = metal_panel(9)
    rect(b, 4, 4, 11, 11, (60, 62, 70))
    rect(b, 6, 6, 9, 9, (200, 60, 60))
    rect(b, 7, 5, 8, 10, (200, 60, 60))
    save("block/bateria_lado", b)

    # Energy duct: a dark insulated block with copper bands; joined faces
    # show a copper socket.
    d = metal_panel(14, (52, 60, 78))
    for i in range(1, 15):
        d[i][4] = d[i][11] = (190, 110, 60, 255)
        d[4][i] = d[11][i] = (190, 110, 60, 255)
    save("block/duto", d)
    c = [row[:] for row in d]
    rect(c, 4, 4, 11, 11, (40, 42, 48))
    rect(c, 5, 5, 10, 10, (205, 120, 66))
    rect(c, 6, 6, 9, 9, (240, 170, 100))
    rect(c, 7, 7, 8, 8, (120, 60, 30))
    save("block/duto_conectado", c)

    # Engineering table: a work top with tools.
    t = blank()
    for y in range(16):
        for x in range(16):
            v = int(noise(x, y, 12) * 18)
            t[y][x] = (120 + v, 84 + v // 2, 50, 255)
    for i in range(16):
        t[0][i] = t[15][i] = t[i][0] = t[i][15] = (70, 72, 80, 255)
    rect(t, 2, 2, 7, 7, (60, 62, 70))
    rect(t, 3, 3, 6, 6, BLUE)
    for i in range(9, 14):
        t[i][i] = (200, 204, 210, 255)
    rect(t, 9, 3, 13, 4, (170, 172, 180))
    save("block/bancada_topo", t)
    l = metal_panel(13)
    rect(l, 1, 1, 14, 4, (130, 92, 54))
    rect(l, 3, 7, 12, 12, DARK)
    for x in range(4, 12, 3):
        rect(l, x, 8, x + 1, 11, (150, 152, 160))
    save("block/bancada_lado", l)


def gui():
    save("gui/vazio", blank())
    # Progress: an arrow filling from the left, 9 steps.
    for step in range(9):
        p = blank()
        shape = []
        for y in range(5, 11):
            for x in range(1, 10):
                shape.append((x, y))
        for i, (y0, y1) in enumerate([(3, 12), (4, 11), (5, 10), (6, 9), (7, 8)]):
            for y in range(y0, y1 + 1):
                shape.append((10 + i, y))
        filled = 1 + step * 14 // 8
        for (x, y) in shape:
            p[y][x] = (90, 160, 255, 255) if x < filled + 1 and step > 0 else (70, 72, 80, 255)
        save(f"gui/progresso_{step}", p)
    # Energy: a bar filling from the bottom, 9 steps.
    for step in range(9):
        p = blank()
        rect(p, 4, 0, 11, 15, (40, 42, 48))
        filled = step * 14 // 8
        for y in range(1, 15):
            on = 15 - y <= filled and step > 0
            for x in range(5, 11):
                p[y][x] = (230, 60, 50, 255) if on and y < 4 else (250, 190, 60, 255) if on and y < 8 \
                    else (80, 210, 90, 255) if on else (60, 62, 70, 255)
        save(f"gui/energia_{step}", p)
    # Flame: the generator's fuel left, 5 steps.
    for step in range(5):
        p = blank()
        for y in range(2, 15):
            half = max(0, (y - 2) * 6 // 12)
            for x in range(8 - half, 8 + half + 1):
                lit = 14 - y < step * 3
                p[y][x] = ((255, 220, 90, 255) if lit and abs(x - 8) < half // 2 else
                           (240, 120, 40, 255) if lit else (70, 72, 80, 255))
        save(f"gui/chama_{step}", p)


if __name__ == "__main__":
    items()
    blocks()
    gui()
    print("textures written to", os.path.normpath(ROOT), file=sys.stderr)
