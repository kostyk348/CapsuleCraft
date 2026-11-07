#!/usr/bin/env python3
"""CapsuleCraft promo v3 — dark neon-pink style, matching the uploaded refs.

Generates: banner_habr_1200x600_ru.png, banner_square_1080x1080_ru.png,
banner_x_1200x675_en.png in assets/promo from the generated capsule art.
"""
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ART = "assets/promo/capsule_neon_1024.png"
OUT = "/home/z/my-project/work/CapsuleCraft/assets/promo"

NOTO_BOLD = "/usr/share/fonts/truetype/chinese/NotoSansSC-Bold.ttf"
NOTO_REG = "/usr/share/fonts/truetype/chinese/NotoSansSC-Regular.ttf"
MONO_BOLD = "/usr/share/fonts/truetype/chinese/SarasaMonoSC-Bold.ttf"
MONO_REG = "/usr/share/fonts/truetype/chinese/LiberationMono-Regular.ttf"

BG = (8, 8, 13)
PANEL = (14, 14, 22)
PINK = (255, 64, 160)
PINK_SOFT = (255, 120, 190)
INK = (240, 240, 248)
MUTED = (150, 150, 168)
FAINT = (96, 96, 112)

VERSION = "v2.9.0"


def font(path, size):
    return ImageFont.truetype(path, size)


def rounded_mask(size, radius):
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius, fill=255)
    return m


def art_tile(side, radius):
    """The capsule art in a rounded tile with a pink neon rim."""
    art = Image.open(ART).convert("RGB").resize((side, side), Image.LANCZOS)
    tile = rounded_mask((side, side), radius)
    out = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    out.paste(art, (0, 0), tile)
    # Neon rim: 3 px pink stroke just inside the tile edge.
    rim = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    d = ImageDraw.Draw(rim)
    d.rounded_rectangle([1, 1, side - 2, side - 2], radius=radius, outline=PINK + (230,), width=3)
    out = Image.alpha_composite(out, rim)
    return out


def glow_layer(size, cx, cy, r, color, alpha):
    """A soft radial glow."""
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color + (alpha,))
    return layer.filter(ImageFilter.GaussianBlur(r * 0.45))


def chip(draw, x, y, text, fnt, border, text_color, pad_x=10, pad_y=6, fill=None):
    bbox = draw.textbbox((0, 0), text, font=fnt)
    w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
    rect = [x, y, x + w + pad_x * 2, y + h + pad_y * 2 + 2]
    if fill:
        draw.rounded_rectangle(rect, radius=7, fill=fill)
    draw.rounded_rectangle(rect, radius=7, outline=border, width=2)
    draw.text((x + pad_x, y + pad_y - bbox[1] + 1), text, font=fnt, fill=text_color)
    return rect[2] - rect[0]


def bullet_block(draw, x, y, lines, fnt, marker_color, text_color, line_h):
    for line in lines:
        draw.rectangle([x, y + 7, x + 9, y + 16], fill=marker_color)
        draw.text((x + 22, y), line, font=fnt, fill=text_color)
        y += line_h
    return y


def build(width, height, lang, out_name, art_side, art_pos, title_size):
    img = Image.new("RGB", (width, height), BG)
    # Background: pink glow behind the art, faint blue one top-left, vignette.
    img = Image.alpha_composite(img.convert("RGBA"), glow_layer((width, height), art_pos[0] + art_side // 2, art_pos[1] + art_side // 2, art_side * 0.62, PINK, 52))
    img = Image.alpha_composite(img, glow_layer((width, height), 140, 60, 260, (60, 80, 220), 26))

    d = ImageDraw.Draw(img)

    f_chip = font(MONO_BOLD, 20)
    f_title = font(NOTO_BOLD, title_size)
    f_tag = font(NOTO_REG, int(title_size * 0.40))
    f_bul = font(MONO_BOLD, 19)
    f_url = font(MONO_REG, 18)

    if lang == "ru":
        tag_lines = ["Ваша база знаний —", "папка с письмами."]
        bullets = [
            "одна страница = один .eml (RFC 5322)",
            "без базы данных · без облака · без аккаунтов",
            "локальный ИИ из каталога: OCR · STT · TTS · co-think",
            "BM25-поиск · планирование · канвас · синхронизация папкой",
        ]
        url = "github.com/kostyk348/CapsuleCraft  ·  Releases: AppImage · Windows · macOS"
    else:
        tag_lines = ["Your knowledge base is", "a folder of .eml files."]
        bullets = [
            "one page = one .eml (RFC 5322)",
            "no database · no cloud · no accounts",
            "local AI catalog: OCR · STT · TTS · co-think",
            "BM25 search · planning · canvas · folder sync",
        ]
        url = "github.com/kostyk348/CapsuleCraft  ·  Releases: AppImage · Windows · macOS"

    left = 64
    y = 56
    # Chips row.
    w1 = chip(d, left, y, "NATIVE · RUST + EGUI", f_chip, FAINT, MUTED, fill=(20, 20, 30))
    chip(d, left + w1 + 14, y, VERSION, f_chip, PINK, PINK_SOFT)
    y += 66

    d.text((left - 4, y), "CapsuleCraft", font=f_title, fill=INK)
    y += int(title_size * 1.18)

    for line in tag_lines:
        d.text((left, y), line, font=f_tag, fill=(206, 206, 222))
        y += int(f_tag.size * 1.34)
    y += 26

    bullet_block(d, left, y, bullets, f_bul, PINK, (176, 176, 194), 34)
    y += 34 * len(bullets) + 18

    d.text((left, height - 64), url, font=f_url, fill=FAINT)

    # Art tile with a soft shadow.
    shadow = glow_layer((width, height), art_pos[0] + art_side // 2, art_pos[1] + art_side // 2, art_side * 0.55, (0, 0, 0), 160)
    img = Image.alpha_composite(img, shadow)
    img.paste(art_tile(art_side, int(art_side * 0.16)), art_pos, art_tile(art_side, int(art_side * 0.16)))

    # Re-draw URL above the art if it overlaps (art is right side; url left) — no-op.
    img.convert("RGB").save(f"{OUT}/{out_name}", "PNG")
    print("saved", out_name)


# Habr banner 1200×600 (RU): art 380px at the right, vertically centered.
build(1200, 600, "ru", "banner_habr_1200x600_ru.png", art_side=380,
      art_pos=(1200 - 380 - 56, (600 - 380) // 2), title_size=92)

# X/Twitter 1200×675 (EN).
build(1200, 675, "en", "banner_x_1200x675_en.png", art_side=400,
      art_pos=(1200 - 400 - 60, (675 - 400) // 2), title_size=96)


def build_square():
    """1080×1080 (RU): centered stack — chips, art, title, tagline, bullets."""
    width = height = 1080
    img = Image.new("RGB", (width, height), BG)
    img = Image.alpha_composite(img.convert("RGBA"), glow_layer((width, height), width // 2, 300, 330, PINK, 54))
    img = Image.alpha_composite(img, glow_layer((width, height), 150, 90, 240, (60, 80, 220), 24))
    d = ImageDraw.Draw(img)

    f_chip = font(MONO_BOLD, 20)
    f_title = font(NOTO_BOLD, 104)
    f_tag = font(NOTO_REG, 40)
    f_bul = font(MONO_BOLD, 22)
    f_url = font(MONO_REG, 18)

    def center_text(y, text, fnt, fill):
        w = d.textlength(text, font=fnt)
        d.text(((width - w) / 2, y), text, font=fnt, fill=fill)

    # Chips row centered.
    chip_w1 = d.textlength("NATIVE · RUST + EGUI", font=f_chip) + 20
    chip_w2 = d.textlength(VERSION, font=f_chip) + 20
    gap = 14
    total = chip_w1 + gap + chip_w2
    cx = (width - total) / 2
    chip(d, cx, 64, "NATIVE · RUST + EGUI", f_chip, FAINT, MUTED, fill=(20, 20, 30))
    chip(d, cx + chip_w1 + gap, 64, VERSION, f_chip, PINK, PINK_SOFT)

    # Art tile centered.
    side = 320
    art_pos = ((width - side) // 2, 140)
    shadow = glow_layer((width, height), width // 2, art_pos[1] + side // 2, side * 0.55, (0, 0, 0), 150)
    img = Image.alpha_composite(img, shadow)
    d = ImageDraw.Draw(img)
    img.paste(art_tile(side, 52), art_pos, art_tile(side, 52))
    d = ImageDraw.Draw(img)

    y = 520
    center_text(y, "CapsuleCraft", f_title, INK)
    y += 132
    center_text(y, "Ваша база знаний —", f_tag, (206, 206, 222))
    y += 54
    center_text(y, "папка с письмами.", f_tag, (206, 206, 222))
    y += 96

    bullets = [
        "▪  одна страница = один .eml (RFC 5322)",
        "▪  без базы данных · без облака · без аккаунтов",
        "▪  локальный ИИ: OCR · STT · TTS · co-think",
        "▪  BM25-поиск · планирование · канвас · синхронизация",
    ]
    for line in bullets:
        center_text(y, line, f_bul, (176, 176, 194))
        y += 40

    center_text(height - 70,
                "github.com/kostyk348/CapsuleCraft  ·  AppImage · Windows · macOS",
                f_url, FAINT)
    img.convert("RGB").save(f"{OUT}/banner_square_1080x1080_ru.png", "PNG")
    print("saved banner_square_1080x1080_ru.png")


build_square()
print("done")
