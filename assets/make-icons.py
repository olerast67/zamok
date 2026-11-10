"""Значок Zamok: синяя плитка, белый замок с корпусом-папкой (zamok.ico) и он же с поднятой
дужкой для открытой папки (zamok-open.ico), плюс PNG 256 для баннера и сайта.

Запуск: python assets/make-icons.py  (нужен Pillow)
Каждый размер рисуется в 8 раз крупнее и уменьшается со сглаживанием; на 16 и 24 точках
дужка и скважина толще, чтобы замок читался в трее и в заголовке окна.
Рисунок задан в сетке 64 на 64, как в исходном SVG (docs/images/logo.svg).
"""
import os
import sys
from PIL import Image, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
TOP = (58, 111, 224)      # #3A6FE0
BOTTOM = (29, 62, 158)    # #1D3E9E
WHITE = (255, 255, 255, 255)
HOLE = BOTTOM + (255,)


def gradient_tile(s: int, radius: float, inset: float) -> Image.Image:
    """Плитка со скруглёнными углами и диагональным переходом цвета."""
    grad = Image.new("RGBA", (s, s))
    px = grad.load()
    for y in range(s):
        for x in range(s):
            t = (x + y) / (2 * (s - 1))
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)) + (255,)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([inset, inset, s - 1 - inset, s - 1 - inset], radius, fill=255)
    out = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    out.paste(grad, (0, 0), mask)
    return out


def logo(size: int, open_: bool) -> Image.Image:
    k = 8
    s = size * k
    u = s / 64  # единица сетки 64x64
    small = size <= 24
    img = gradient_tile(s, 14 * u, 2 * u if size > 24 else 0.5 * u)
    d = ImageDraw.Draw(img)

    # Дужка: полукольцо радиуса 10 с центром (32, 23) и две ножки до корпуса
    w = (7.5 if small else 5.5) * u
    cx, cy, r = 32 * u, 23 * u, 10 * u
    lift = 6 * u if open_ else 0
    d.arc([cx - r, cy - r - lift, cx + r, cy + r - lift], 180, 360, fill=WHITE, width=round(w))
    leg_bottom = 30 * u
    # Левая ножка у открытого замка короче: дужка вышла из корпуса
    d.rectangle([cx - r, cy - lift, cx - r + w, (cy - lift + 3 * u) if open_ else leg_bottom], fill=WHITE)
    d.rectangle([cx + r - w, cy - lift, cx + r, leg_bottom], fill=WHITE)

    # Корпус в форме папки: язычок слева сверху, затем прямоугольник
    tab = [15 * u, 28 * u, 28 * u, 33 * u]
    d.rounded_rectangle(tab, 2.5 * u, fill=WHITE)
    body = [15 * u, 31 * u, 49 * u, 52 * u]
    d.rounded_rectangle(body, 4 * u, fill=WHITE)

    # Скважина
    hr = (4.2 if small else 3.2) * u
    hx, hy = 32 * u, 40 * u
    d.ellipse([hx - hr, hy - hr, hx + hr, hy + hr], fill=HOLE)
    hw = (2.0 if small else 1.4) * u
    d.rounded_rectangle([hx - hw, hy, hx + hw, 47.5 * u], 1.2 * u, fill=HOLE)

    return img.resize((size, size), Image.LANCZOS)


def build(out_dir: str) -> None:
    for open_, name in ((False, "zamok"), (True, "zamok-open")):
        frames = [logo(n, open_) for n in SIZES]
        frames[-1].save(os.path.join(out_dir, f"{name}-256.png"))
        frames[-1].save(os.path.join(out_dir, f"{name}.ico"), sizes=[(n, n) for n in SIZES],
                        append_images=frames[:-1])


if __name__ == "__main__":
    build(sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__)))
