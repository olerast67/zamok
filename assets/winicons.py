"""Достать значок из системной библиотеки Windows (imageres.dll, shell32.dll) в нужном размере.

Значки Zamok собираются из значков самой Windows на компьютере, где ставится программа:
на Windows 10 выходит папка из Windows 10, на Windows 11 из Windows 11. Рисунки Microsoft
в репозиторий не попадают.

Лист для выбора значков: python assets/winicons.py sheet imageres.dll out.png
"""
import ctypes
import os
import sys
from ctypes import wintypes

from PIL import Image, ImageDraw

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)


class ICONINFO(ctypes.Structure):
    _fields_ = [("fIcon", wintypes.BOOL), ("xHotspot", wintypes.DWORD), ("yHotspot", wintypes.DWORD),
                ("hbmMask", wintypes.HBITMAP), ("hbmColor", wintypes.HBITMAP)]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
                ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD)]


user32.PrivateExtractIconsW.argtypes = [wintypes.LPCWSTR, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                        ctypes.POINTER(wintypes.HICON), ctypes.POINTER(wintypes.UINT),
                                        wintypes.UINT, wintypes.UINT]
user32.GetIconInfo.argtypes = [wintypes.HICON, ctypes.POINTER(ICONINFO)]
user32.DestroyIcon.argtypes = [wintypes.HICON]
user32.GetDC.restype = wintypes.HDC
user32.GetDC.argtypes = [wintypes.HWND]
user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
gdi32.GetDIBits.argtypes = [wintypes.HDC, wintypes.HBITMAP, wintypes.UINT, wintypes.UINT, ctypes.c_void_p,
                            ctypes.c_void_p, wintypes.UINT]
gdi32.DeleteObject.argtypes = [wintypes.HGDIOBJ]


def system_dll(name: str) -> str:
    return os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32", name)


def count(dll: str) -> int:
    return user32.PrivateExtractIconsW(dll, 0, 0, 0, None, None, 0, 0)


def extract(dll: str, index: int, size: int) -> Image.Image | None:
    """Значок номер `index` (по порядку, с нуля) ровно в размере `size`: Windows берёт
    подходящую картинку из файла, а не растягивает чужую."""
    h = wintypes.HICON()
    got = user32.PrivateExtractIconsW(dll, index, size, size, ctypes.byref(h), None, 1, 0)
    if got != 1 or not h:
        return None
    try:
        info = ICONINFO()
        if not user32.GetIconInfo(h, ctypes.byref(info)):
            return None
        try:
            bmi = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), size, -size, 1, 32, 0, 0, 0, 0, 0, 0)
            buf = (ctypes.c_ubyte * (size * size * 4))()
            dc = user32.GetDC(None)
            try:
                if not gdi32.GetDIBits(dc, info.hbmColor, 0, size, buf, ctypes.byref(bmi), 0):
                    return None
            finally:
                user32.ReleaseDC(None, dc)
            img = Image.frombuffer("RGBA", (size, size), bytes(buf), "raw", "BGRA", 0, 1)
            if img.getchannel("A").getextrema() == (0, 0):
                # Старые значки без альфа-канала: прозрачность из маски
                mbuf = (ctypes.c_ubyte * (size * size * 4))()
                dc = user32.GetDC(None)
                gdi32.GetDIBits(dc, info.hbmMask, 0, size, mbuf, ctypes.byref(bmi), 0)
                user32.ReleaseDC(None, dc)
                mask = Image.frombuffer("RGBA", (size, size), bytes(mbuf), "raw", "BGRA", 0, 1).convert("L")
                img.putalpha(mask.point(lambda v: 0 if v else 255))
            return img.copy()
        finally:
            if info.hbmColor:
                gdi32.DeleteObject(info.hbmColor)
            if info.hbmMask:
                gdi32.DeleteObject(info.hbmMask)
    finally:
        user32.DestroyIcon(h)


def sheet(dll: str, out: str, size: int = 32, start: int = 0, end: int | None = None):
    n = count(dll)
    end = min(n, end or n)
    cols = 20
    cell = size + 28
    rows = (end - start + cols - 1) // cols
    img = Image.new("RGBA", (cols * cell, rows * cell), (240, 240, 240, 255))
    d = ImageDraw.Draw(img)
    for i in range(start, end):
        icon = extract(dll, i, size)
        x, y = ((i - start) % cols) * cell, ((i - start) // cols) * cell
        if icon:
            img.alpha_composite(icon, (x + 14, y + 2))
        d.text((x + 4, y + size + 4), str(i), fill=(0, 0, 0, 255))
    img.save(out)
    print(f"{dll}: {n} icons, sheet {start}..{end - 1} -> {out}")


if __name__ == "__main__":
    if sys.argv[1] == "sheet":
        dll = sys.argv[2] if os.path.isabs(sys.argv[2]) else system_dll(sys.argv[2])
        start = int(sys.argv[4]) if len(sys.argv) > 4 else 0
        end = int(sys.argv[5]) if len(sys.argv) > 5 else None
        sheet(dll, sys.argv[3], 32, start, end)
