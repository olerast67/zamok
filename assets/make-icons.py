"""Значок Zamok: синяя плитка и белый симметричный замок (zamok.ico), он же с поднятой
дужкой для открытой папки (zamok-open.ico), плюс PNG 256 для баннера и сайта.

Запуск: python assets/make-icons.py [папка]  (нужен Pillow)
Каждый размер рисуется в 8 раз крупнее и уменьшается со сглаживанием; на 24 точках и меньше
дужка и скважина толще, чтобы замок читался в трее и в заголовке окна.
Рисунок задан в сетке 64 на 64, как в docs/images/logo.svg. Дужка строится как кольцо
(внешний круг минус внутренний) и две ножки той же ширины, поэтому стыки ровные.
"""
import os
import sys
from PIL import Image, ImageChops, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
TOP = (58, 111, 224)      # #3A6FE0
BOTTOM = (29, 62, 158)    # #1D3E9E
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


def lock_mask(s: int, u: float, small: bool, open_: bool) -> Image.Image:
    """Белая часть рисунка как маска: дужка, корпус, минус скважина."""
    m = Image.new("L", (s, s), 0)
    d = ImageDraw.Draw(m)
    cx = 32 * u
    w = (7.0 if small else 5.5) * u          # толщина дужки
    ro = 11 * u                              # внешний радиус дужки
    ri = ro - w
    cy = (23 - (6 if open_ else 0)) * u      # центр полукольца
    body_top = 30 * u
    # Кольцо: верхняя половина
    ring = Image.new("L", (s, s), 0)
    rd = ImageDraw.Draw(ring)
    rd.ellipse([cx - ro, cy - ro, cx + ro, cy + ro], fill=255)
    rd.ellipse([cx - ri, cy - ri, cx + ri, cy + ri], fill=0)
    rd.rectangle([0, cy, s, s], fill=0)
    m = ImageChops.lighter(m, ring)
    d = ImageDraw.Draw(m)
    # Ножки ровно под кольцом, обе до корпуса; у открытого замка левая обрывается
    d.rectangle([cx + ri, cy - 1, cx + ro, body_top + 2 * u], fill=255)
    left_end = cy + 3 * u if open_ else body_top + 2 * u
    d.rectangle([cx - ro, cy - 1, cx - ri, left_end], fill=255)
    # Корпус
    d.rounded_rectangle([15 * u, body_top, 49 * u, 52 * u], 5 * u, fill=255)
    # Скважина
    hr = (4.2 if small else 3.4) * u
    hx, hy = cx, 39.5 * u
    d.ellipse([hx - hr, hy - hr, hx + hr, hy + hr], fill=0)
    hw = (2.0 if small else 1.5) * u
    d.rounded_rectangle([hx - hw, hy, hx + hw, 46.5 * u], hw * 0.8, fill=0)
    return m


def logo(size: int, open_: bool) -> Image.Image:
    k = 8
    s = size * k
    u = s / 64
    small = size <= 24
    img = gradient_tile(s, 14 * u, 2 * u if size > 24 else 0.5 * u)
    white = Image.new("RGBA", (s, s), (255, 255, 255, 255))
    img.paste(white, (0, 0), lock_mask(s, u, small, open_))
    return img.resize((size, size), Image.LANCZOS)


def build(out_dir: str) -> None:
    for open_, name in ((False, "zamok"), (True, "zamok-open")):
        frames = [logo(n, open_) for n in SIZES]
        frames[-1].save(os.path.join(out_dir, f"{name}-256.png"))
        frames[-1].save(os.path.join(out_dir, f"{name}.ico"), sizes=[(n, n) for n in SIZES],
                        append_images=frames[:-1])


if __name__ == "__main__":
    build(sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__)))
