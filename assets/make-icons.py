"""Значки Zamok из кода: замок закрытый (zamok.ico) и с открытой дужкой (zamok-open.ico).

Запуск: python assets/make-icons.py  (нужен Pillow)
Каждый размер рисуется отдельно в 4 раза крупнее и уменьшается со сглаживанием;
на 16 и 24 пикселях линии толще, чтобы замок читался в трее.
Это временные значки альфы: настоящие (папка с замком в стиле Windows 11) рисуются на этапе 6.
"""
import os
from PIL import Image, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
BODY = (43, 58, 85, 255)        # тёмно-синий корпус
EDGE = (232, 236, 241, 255)     # светлый край: виден и на тёмной панели задач
SHACKLE = (150, 162, 176, 255)  # серая дужка
KEYHOLE = (232, 236, 241, 255)


def lock(size: int, open_: bool) -> Image.Image:
    k = 4
    s = size * k
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    small = size <= 24
    edge = max(k, round(s * (0.06 if small else 0.035)))
    shackle_w = round(s * (0.13 if small else 0.10))

    # Дужка: дуга над корпусом; у открытого замка поднята и сдвинута вправо
    left, right = s * 0.27, s * 0.73
    top = s * (0.04 if open_ else 0.08)
    bottom = s * 0.58
    dx = s * 0.16 if open_ else 0
    box = [left + dx, top, right + dx, bottom]
    d.arc(box, 180, 360, fill=EDGE, width=shackle_w + 2 * edge)
    d.arc([box[0] + edge, box[1] + edge, box[2] - edge, box[3] - edge], 180, 360,
          fill=SHACKLE, width=shackle_w)
    leg_top = (top + bottom) / 2
    leg_bottom = s * 0.50
    for x in ([left + dx, right + dx] if not open_ else [right + dx]):
        x0 = x if x == left + dx else x - shackle_w - 2 * edge
        d.rectangle([x0, leg_top, x0 + shackle_w + 2 * edge, leg_bottom], fill=EDGE)
        d.rectangle([x0 + edge, leg_top, x0 + edge + shackle_w, leg_bottom], fill=SHACKLE)

    # Корпус со светлым краем и закруглёнными углами
    body = [s * 0.14, s * 0.44, s * 0.86, s * 0.96]
    r = s * 0.10
    d.rounded_rectangle(body, r, fill=EDGE)
    d.rounded_rectangle([body[0] + edge, body[1] + edge, body[2] - edge, body[3] - edge],
                        max(1, r - edge), fill=BODY)

    # Замочная скважина
    cx, cy = s * 0.5, s * 0.66
    hr = s * (0.085 if small else 0.065)
    d.ellipse([cx - hr, cy - hr, cx + hr, cy + hr], fill=KEYHOLE)
    if not small:
        d.polygon([(cx - hr * 0.55, cy), (cx + hr * 0.55, cy),
                   (cx + hr * 0.8, s * 0.84), (cx - hr * 0.8, s * 0.84)], fill=KEYHOLE)
    return img.resize((size, size), Image.LANCZOS)


here = os.path.dirname(os.path.abspath(__file__))
for name, open_ in (("zamok.ico", False), ("zamok-open.ico", True)):
    images = [lock(n, open_) for n in SIZES]
    images[-1].save(os.path.join(here, name), sizes=[(n, n) for n in SIZES],
                    append_images=images[:-1])
    images[-1].save(os.path.join(here, name.replace(".ico", "-256.png")))
print("ok")
