"""Regenerate the embedded PSP font and XMB icon (Pillow, CairoSVG)."""
from pathlib import Path
from io import BytesIO
import shutil
import struct
from PIL import Image, ImageDraw, ImageFont
import cairosvg

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"
ASSETS.mkdir(exist_ok=True)
FONT = "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf"
CHARS = "".join(chr(i) for i in range(32, 383)) + "…↑↓←→●○◆□△▼×›–—•"
# Each glyph: codepoint u32, advance u8, followed by 18x18 alpha bytes.
for size in (12, 14):
    font = ImageFont.truetype(FONT, size)
    data = bytearray()
    for char in CHARS:
        tile = Image.new("L", (18, 18))
        ImageDraw.Draw(tile).text((1, 0), char, font=font, fill=255)
        data += struct.pack("<IB", ord(char), round(font.getlength(char)))
        data += tile.tobytes()
    (ASSETS / f"font{size}.bin").write_bytes(data)
shutil.copyfile("/usr/share/licenses/dejavu-sans-fonts/LICENSE", ASSETS / "FONT-LICENSE.txt")
svg = (ROOT.parents[1] / "assets/prod/logo.svg").read_text()
svg = svg.replace('fill="black"', 'fill="#10171f"').replace('fill="white"', 'fill="#a3cbff"')
logo = Image.open(BytesIO(cairosvg.svg2png(bytestring=svg.encode(), output_width=76, output_height=76)))
icon = Image.new("RGB", (144, 80), "#10171f")
icon.paste(logo, (1, 2), logo)
draw = ImageDraw.Draw(icon)
draw.text((80, 29), "PSP", font=ImageFont.truetype(FONT, 21), fill="#f3f5fa")
icon.save(ASSETS / "ICON0.png")
