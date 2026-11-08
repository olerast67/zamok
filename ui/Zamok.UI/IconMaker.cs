using System.Runtime.InteropServices;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Zamok.UI;

/// <summary>
/// Значки Zamok из значков Windows этого компьютера (<c>Zamok.exe --make-icons &lt;папка&gt;</c>,
/// запускается при установке). Из imageres.dll берутся системная папка (3) и золотой замок (54):
/// на Windows 10 выходят значки Windows 10, на Windows 11 значки 11. Каждый размер достаётся из
/// библиотеки готовым, поэтому значки чёткие. Рисунки Microsoft в репозиторий не попадают.
/// </summary>
internal static partial class IconMaker
{
    private const int Folder = 3, Lock = 54;
    private static readonly int[] Sizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];
    private static readonly int[] TraySizes = [16, 20, 24, 32, 40, 48];

    /// <summary>Картинка BGRA с обычной (не умноженной) прозрачностью.</summary>
    private sealed class Image(int size)
    {
        public readonly int Size = size;
        public readonly byte[] Px = new byte[size * size * 4];
    }

    public static async Task MakeAllAsync(string outDir)
    {
        Directory.CreateDirectory(outDir);
        var dll = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "imageres.dll");
        var locked = Sizes.Select(s => FolderWith(dll, s, open: false)).ToList();
        var opened = Sizes.Select(s => FolderWith(dll, s, open: true)).ToList();
        await SaveIcoAsync(locked, Path.Combine(outDir, "folder-locked.ico"));
        await SaveIcoAsync(opened, Path.Combine(outDir, "folder-open.ico"));
        await SaveIcoAsync([.. Sizes.Select(s => Extract(dll, Lock, s))], Path.Combine(outDir, "open-item.ico"));
        await SaveIcoAsync([.. locked.Where(i => TraySizes.Contains(i.Size))], Path.Combine(outDir, "tray-locked.ico"));
        await SaveIcoAsync([.. opened.Where(i => TraySizes.Contains(i.Size))], Path.Combine(outDir, "tray-open.ico"));
        // Для окон: WinUI растягивает маленькую картинку из .ico, PNG нужного размера чёткий
        await File.WriteAllBytesAsync(Path.Combine(outDir, "folder-locked-64.png"),
            await PngAsync(locked.First(i => i.Size == 64)));
    }

    private static int BadgeSize(int size) => size <= 16 ? 10 : size <= 24 ? (int)Math.Round(size * 0.6) : (int)Math.Round(size * 0.55);

    /// <summary>Системная папка с замком в правом нижнем углу, как у дисков BitLocker.</summary>
    private static Image FolderWith(string dll, int size, bool open)
    {
        var img = Extract(dll, Folder, size);
        var b = BadgeSize(size);
        var badge = open ? OpenLock(dll, b) : Extract(dll, Lock, b);
        var inset = size <= 24 ? 0 : (int)Math.Round(size * 0.02);
        Over(img, badge, 0, 0, b, b, size - b - inset, size - b - inset);
        return img;
    }

    /// <summary>Открытый замок из системного закрытого: дужка выше крышки поднята, виден зазор.</summary>
    private static Image OpenLock(string dll, int size)
    {
        var l = Math.Max(8, (int)Math.Round(size * 0.84));
        var lockImg = Extract(dll, Lock, l);
        var cut = (int)Math.Round(l * 0.36);
        var lift = (int)Math.Round(size * 0.16);
        var img = new Image(size);
        int ox = (size - l) / 2, oy = size - l;
        Over(img, lockImg, 0, cut, l, l - cut, ox, oy + cut);   // корпус на месте
        Over(img, lockImg, 0, 0, l, cut, ox, oy - lift);        // дужка поднята
        return img;
    }

    /// <summary>Наложить часть <paramref name="src"/> поверх <paramref name="dst"/> (альфа-смешивание).</summary>
    private static void Over(Image dst, Image src, int sx, int sy, int w, int h, int dx, int dy)
    {
        for (var y = 0; y < h; y++)
        for (var x = 0; x < w; x++)
        {
            int tx = dx + x, ty = dy + y, fx = sx + x, fy = sy + y;
            if (tx < 0 || ty < 0 || tx >= dst.Size || ty >= dst.Size || fx >= src.Size || fy >= src.Size) continue;
            var si = (fy * src.Size + fx) * 4;
            var di = (ty * dst.Size + tx) * 4;
            double sa = src.Px[si + 3] / 255.0, da = dst.Px[di + 3] / 255.0;
            var oa = sa + da * (1 - sa);
            if (oa <= 0) continue;
            for (var c = 0; c < 3; c++)
                dst.Px[di + c] = (byte)Math.Round((src.Px[si + c] * sa + dst.Px[di + c] * da * (1 - sa)) / oa);
            dst.Px[di + 3] = (byte)Math.Round(oa * 255);
        }
    }

    /// <summary>Значок номер <paramref name="index"/> ровно в размере <paramref name="size"/>.</summary>
    private static Image Extract(string dll, int index, int size)
    {
        var img = new Image(size);
        var icons = new IntPtr[1];
        if (PrivateExtractIconsW(dll, index, size, size, icons, IntPtr.Zero, 1, 0) != 1 || icons[0] == IntPtr.Zero)
            return img;
        try
        {
            if (!GetIconInfo(icons[0], out var info)) return img;
            try
            {
                var bmi = new BITMAPINFOHEADER
                {
                    biSize = Marshal.SizeOf<BITMAPINFOHEADER>(), biWidth = size, biHeight = -size,
                    biPlanes = 1, biBitCount = 32,
                };
                var dc = GetDC(IntPtr.Zero);
                try
                {
                    GetDIBits(dc, info.hbmColor, 0, (uint)size, img.Px, ref bmi, 0);
                    // Старые значки без прозрачности: взять её из маски
                    if (img.Px.Where((_, i) => i % 4 == 3).All(a => a == 0))
                    {
                        var mask = new byte[size * size * 4];
                        GetDIBits(dc, info.hbmMask, 0, (uint)size, mask, ref bmi, 0);
                        for (var i = 0; i < size * size; i++) img.Px[i * 4 + 3] = mask[i * 4] == 0 ? (byte)255 : (byte)0;
                    }
                }
                finally { ReleaseDC(IntPtr.Zero, dc); }
            }
            finally
            {
                DeleteObject(info.hbmColor);
                DeleteObject(info.hbmMask);
            }
        }
        finally { DestroyIcon(icons[0]); }
        return img;
    }

    private static async Task<byte[]> PngAsync(Image img)
    {
        using var stream = new InMemoryRandomAccessStream();
        var enc = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        enc.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Straight,
            (uint)img.Size, (uint)img.Size, 96, 96, img.Px);
        await enc.FlushAsync();
        var bytes = new byte[stream.Size];
        using var reader = new DataReader(stream.GetInputStreamAt(0));
        await reader.LoadAsync((uint)stream.Size);
        reader.ReadBytes(bytes);
        return bytes;
    }

    /// <summary>Файл .ico с картинками PNG внутри (так хранит значки и сама Windows).</summary>
    private static async Task SaveIcoAsync(List<Image> images, string path)
    {
        var pngs = new List<byte[]>();
        foreach (var i in images) pngs.Add(await PngAsync(i));
        using var ms = new MemoryStream();
        using var w = new BinaryWriter(ms);
        w.Write((ushort)0); w.Write((ushort)1); w.Write((ushort)images.Count);
        var offset = 6 + 16 * images.Count;
        for (var k = 0; k < images.Count; k++)
        {
            var s = images[k].Size;
            w.Write((byte)(s >= 256 ? 0 : s)); w.Write((byte)(s >= 256 ? 0 : s));
            w.Write((byte)0); w.Write((byte)0);
            w.Write((ushort)1); w.Write((ushort)32);
            w.Write(pngs[k].Length); w.Write(offset);
            offset += pngs[k].Length;
        }
        foreach (var p in pngs) w.Write(p);
        w.Flush();
        await File.WriteAllBytesAsync(path, ms.ToArray());
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ICONINFO { public int fIcon, xHotspot, yHotspot; public IntPtr hbmMask, hbmColor; }

    [StructLayout(LayoutKind.Sequential)]
    private struct BITMAPINFOHEADER
    {
        public int biSize, biWidth, biHeight;
        public short biPlanes, biBitCount;
        public int biCompression, biSizeImage, biXPelsPerMeter, biYPelsPerMeter, biClrUsed, biClrImportant;
    }

    [LibraryImport("user32.dll", StringMarshalling = StringMarshalling.Utf16)]
    private static partial uint PrivateExtractIconsW(string file, int index, int cx, int cy,
        [Out] IntPtr[] icons, IntPtr ids, uint count, uint flags);
    [LibraryImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetIconInfo(IntPtr icon, out ICONINFO info);
    [LibraryImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool DestroyIcon(IntPtr icon);
    [LibraryImport("user32.dll")] private static partial IntPtr GetDC(IntPtr hwnd);
    [LibraryImport("user32.dll")] private static partial int ReleaseDC(IntPtr hwnd, IntPtr dc);
    [LibraryImport("gdi32.dll")]
    private static partial int GetDIBits(IntPtr dc, IntPtr bmp, uint start, uint lines, [Out] byte[] bits,
        ref BITMAPINFOHEADER bmi, uint usage);
    [LibraryImport("gdi32.dll")] [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool DeleteObject(IntPtr obj);
}
