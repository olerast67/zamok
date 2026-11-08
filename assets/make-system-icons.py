"""Собрать значки Zamok из значков Windows этого компьютера (запускается при установке).

python assets/make-system-icons.py <папка>

В папке появляются:
- folder-locked.ico  системная папка с системным золотым замком в углу (запертая папка);
- open-item.ico      системный замок (значок «Открыть <Имя>.zamok»);
- tray-locked.ico, tray-open.ico  значки дисков BitLocker с закрытым и открытым замком (трей).

Берутся из imageres.dll: на Windows 10 выходят значки Windows 10, на Windows 11 значки
Windows 11. Каждый размер достаётся из библиотеки ровно в этом размере (Windows хранит
отдельные картинки под размеры), поэтому значки чёткие. Рисунки Microsoft в репозиторий
не попадают, значки собираются на компьютере человека.
"""
import os
import sys

from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from winicons import extract, system_dll  # noqa: E402

FOLDER, LOCK, DRIVE_UNLOCKED, DRIVE_LOCKED = 3, 54, 209, 210
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
TRAY_SIZES = [16, 20, 24, 32, 40, 48]


def badge_size(size: int) -> int:
    # Мелким значкам замок покрупнее, иначе его не разобрать
    if size <= 16:
        return 10
    if size <= 24:
        return round(size * 0.6)
    return round(size * 0.55)


def folder_locked(dll: str, size: int) -> Image.Image:
    base = extract(dll, FOLDER, size)
    b = badge_size(size)
    lock = extract(dll, LOCK, b)
    img = base.copy()
    # Замок в правом нижнем углу, как у дисков BitLocker; у самых мелких вплотную к краю
    inset = 0 if size <= 24 else round(size * 0.02)
    img.alpha_composite(lock, (size - b - inset, size - b - inset))
    return img


def save_ico(frames: list[Image.Image], path: str):
    frames = sorted(frames, key=lambda f: f.size[0])
    big = frames[-1]
    big.save(path, format="ICO", sizes=[f.size for f in frames], append_images=frames[:-1])


def main(out: str):
    os.makedirs(out, exist_ok=True)
    dll = system_dll("imageres.dll")
    save_ico([folder_locked(dll, s) for s in SIZES], os.path.join(out, "folder-locked.ico"))
    save_ico([extract(dll, LOCK, s) for s in SIZES], os.path.join(out, "open-item.ico"))
    save_ico([extract(dll, DRIVE_LOCKED, s) for s in TRAY_SIZES], os.path.join(out, "tray-locked.ico"))
    save_ico([extract(dll, DRIVE_UNLOCKED, s) for s in TRAY_SIZES], os.path.join(out, "tray-open.ico"))
    print("ok")


if __name__ == "__main__":
    main(sys.argv[1])
