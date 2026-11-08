#!/usr/bin/env python3
import math
import pathlib
import shutil
import struct
import subprocess
import tempfile
import zlib


def _chunk(tag, data):
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def _draw_icon(size):
    scale = 2
    side = size * scale
    pixels = bytearray(side * side * 4)

    for y in range(side):
        t = y / max(1, side - 1)
        color = bytes((7 + int(4 * t), 13 + int(7 * (1 - t)), 27 + int(13 * (1 - t)), 255))
        row = color * side
        start = y * side * 4
        pixels[start:start + len(row)] = row

    def ring(cx_ratio, cy_ratio, radius_ratio, width_ratio, gap_degrees, color):
        cx = cx_ratio * side
        cy = cy_ratio * side
        radius = radius_ratio * side
        half_width = width_ratio * side / 2
        extent = radius + half_width + 2
        x0 = max(0, int(cx - extent))
        x1 = min(side, int(cx + extent + 1))
        y0 = max(0, int(cy - extent))
        y1 = min(side, int(cy + extent + 1))
        for y in range(y0, y1):
            dy = y + 0.5 - cy
            for x in range(x0, x1):
                dx = x + 0.5 - cx
                distance = math.hypot(dx, dy)
                edge = half_width - abs(distance - radius)
                if edge <= -0.5:
                    continue
                angle = math.degrees(math.atan2(dy, dx))
                delta = abs((angle - gap_degrees + 180) % 360 - 180)
                if delta < 35:
                    continue
                alpha = min(1.0, max(0.0, edge + 0.5))
                offset = (y * side + x) * 4
                for channel in range(3):
                    pixels[offset + channel] = int(
                        pixels[offset + channel] * (1 - alpha) + color[channel] * alpha
                    )

    def dot(cx_ratio, cy_ratio, radius_ratio, color):
        cx = cx_ratio * side
        cy = cy_ratio * side
        radius = radius_ratio * side
        x0 = max(0, int(cx - radius - 1))
        x1 = min(side, int(cx + radius + 2))
        y0 = max(0, int(cy - radius - 1))
        y1 = min(side, int(cy + radius + 2))
        for y in range(y0, y1):
            for x in range(x0, x1):
                distance = math.hypot(x + 0.5 - cx, y + 0.5 - cy)
                alpha = min(1.0, max(0.0, radius + 0.5 - distance))
                if alpha <= 0:
                    continue
                offset = (y * side + x) * 4
                for channel in range(3):
                    pixels[offset + channel] = int(
                        pixels[offset + channel] * (1 - alpha) + color[channel] * alpha
                    )

    cyan = (35, 195, 216)
    gold = (255, 190, 48)
    ring(0.405, 0.59, 0.255, 0.076, -45, cyan)
    ring(0.595, 0.405, 0.255, 0.076, 135, gold)
    dot(0.405, 0.59, 0.064, cyan)
    dot(0.595, 0.405, 0.064, gold)

    output = bytearray()
    for y in range(size):
        output.append(0)
        for x in range(size):
            source = ((y * scale) * side + x * scale) * 4
            for channel in range(4):
                total = sum(
                    pixels[source + dy * side * 4 + dx * 4 + channel]
                    for dy in range(scale)
                    for dx in range(scale)
                )
                output.append(total // (scale * scale))
    return bytes(output)


def png(size):
    image = _draw_icon(size)
    return (
        b"\x89PNG\r\n\x1a\n"
        + _chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + _chunk(b"IDAT", zlib.compress(image, 9))
        + _chunk(b"IEND", b"")
    )


root = pathlib.Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
root.mkdir(parents=True, exist_ok=True)
sizes = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 1024,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}
for filename, size in sizes.items():
    (root / filename).write_bytes(png(size))

png32 = png(32)
ico = struct.pack("<HHH", 0, 1, 1) + struct.pack(
    "<BBBBHHII", 32, 32, 0, 0, 1, 32, len(png32), 22
) + png32
(root / "icon.ico").write_bytes(ico)

if shutil.which("iconutil"):
    with tempfile.TemporaryDirectory() as temporary:
        iconset = pathlib.Path(temporary) / "Nebula.iconset"
        iconset.mkdir()
        for filename, size in [
            ("icon_16x16.png", 16),
            ("icon_16x16@2x.png", 32),
            ("icon_32x32.png", 32),
            ("icon_32x32@2x.png", 64),
            ("icon_128x128.png", 128),
            ("icon_128x128@2x.png", 256),
            ("icon_256x256.png", 256),
            ("icon_256x256@2x.png", 512),
            ("icon_512x512.png", 512),
            ("icon_512x512@2x.png", 1024),
        ]:
            (iconset / filename).write_bytes(png(size))
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(root / "icon.icns")],
            check=True,
        )

print("wrote Nebula app icons to", root)
