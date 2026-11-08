using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Microsoft.Windows.ApplicationModel.Resources;
using Windows.Graphics;

namespace Zamok.UI;

internal static partial class WindowHelper
{
    /// <summary>
    /// Маленькое окно по центру экрана: без изменения размера и разворачивания,
    /// фон Mica на Windows 11 (на Windows 10 обычный фон темы).
    /// </summary>
    public static void SetupDialog(Window window, int width, int height)
    {
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(window);
        var scale = GetDpiForWindow(hwnd) / 96.0;
        var appWindow = window.AppWindow;
        var size = new SizeInt32((int)(width * scale), (int)(height * scale));
        var area = DisplayArea.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd),
            DisplayAreaFallback.Primary).WorkArea;
        appWindow.MoveAndResize(new RectInt32(
            area.X + (area.Width - size.Width) / 2,
            area.Y + (area.Height - size.Height) / 2,
            size.Width, size.Height));
        if (appWindow.Presenter is OverlappedPresenter p)
        {
            p.IsResizable = false;
            p.IsMaximizable = false;
            p.IsMinimizable = false;
        }
        window.Title = "Zamok";
        if (MicaController.IsSupported())
            window.SystemBackdrop = new MicaBackdrop();
    }

    public static IntPtr Handle(Window window) => WinRT.Interop.WindowNative.GetWindowHandle(window);

    [LibraryImport("user32.dll")]
    private static partial uint GetDpiForWindow(IntPtr hwnd);
}

/// <summary>Строки интерфейса из Strings\&lt;язык&gt;\Resources.resw.</summary>
internal static class Strings
{
    private static readonly ResourceLoader Loader = new();

    public static string Get(string key)
    {
        try
        {
            var s = Loader.GetString(key);
            return string.IsNullOrEmpty(s) ? key : s;
        }
        catch
        {
            return key;
        }
    }

    public static string Format(string key, params object[] args) =>
        string.Format(Get(key), args);

    /// <summary>Человеческий текст для кода ошибки core (Error_&lt;код&gt;).</summary>
    public static string Error(string? code) =>
        Get("Error_" + (code ?? "internal"));
}
