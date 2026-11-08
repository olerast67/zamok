using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Windows.ApplicationModel.Resources;
using Windows.Graphics;

namespace Zamok.UI;

internal static partial class WindowHelper
{
    /// <summary>
    /// Маленькое окно как системный диалог: по центру экрана, без изменения размера,
    /// заголовок в цветах темы (своя полоса с значком вместо белой системной),
    /// фон Mica на Windows 11 и обычный фон темы на Windows 10.
    /// </summary>
    public static void SetupDialog(Window window, Panel root, UIElement titleBar, Image titleIcon,
        int width, int height)
    {
        var appWindow = window.AppWindow;
        window.Title = "Zamok";
        window.ExtendsContentIntoTitleBar = true;
        window.SetTitleBar(titleBar);
        var dark = root.ActualTheme == ElementTheme.Dark ||
                   (root.ActualTheme == ElementTheme.Default && Application.Current.RequestedTheme == ApplicationTheme.Dark);
        var tb = appWindow.TitleBar;
        tb.ButtonBackgroundColor = Colors.Transparent;
        tb.ButtonInactiveBackgroundColor = Colors.Transparent;
        tb.ButtonForegroundColor = dark ? Colors.White : Colors.Black;
        tb.ButtonHoverBackgroundColor = dark ? Windows.UI.Color.FromArgb(0x20, 0xFF, 0xFF, 0xFF)
                                             : Windows.UI.Color.FromArgb(0x20, 0, 0, 0);
        tb.ButtonHoverForegroundColor = tb.ButtonForegroundColor;

        if (appWindow.Presenter is OverlappedPresenter p)
        {
            p.IsResizable = false;
            p.IsMaximizable = false;
            p.IsMinimizable = false;
        }
        var icon = AppIconPath();
        if (icon is not null)
        {
            appWindow.SetIcon(icon);
            titleIcon.Source = new BitmapImage(new Uri(icon)) { DecodePixelWidth = 16 };
        }
        if (MicaController.IsSupported())
        {
            window.SystemBackdrop = new MicaBackdrop();
            root.Background = new SolidColorBrush(Colors.Transparent);
        }
        Resize(window, width, height);
    }

    /// <summary>Поменять размер окна (в логических точках) и поставить его по центру экрана.</summary>
    public static void Resize(Window window, int width, int height)
    {
        var hwnd = Handle(window);
        var scale = GetDpiForWindow(hwnd) / 96.0;
        var size = new SizeInt32((int)(width * scale), (int)(height * scale));
        var area = DisplayArea.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd),
            DisplayAreaFallback.Primary).WorkArea;
        window.AppWindow.MoveAndResize(new RectInt32(
            area.X + (area.Width - size.Width) / 2,
            area.Y + (area.Height - size.Height) / 2,
            size.Width, size.Height));
    }

    /// <summary>Системный замок, собранный при установке, или запасной свой значок.</summary>
    public static string? AppIconPath() =>
        new[] { "open-item.ico", "zamok.ico" }
            .Select(n => Path.Combine(AppContext.BaseDirectory, n)).FirstOrDefault(File.Exists);

    /// <summary>Значок запертой папки Zamok (системная папка с замком), если он собран.</summary>
    public static ImageSource? FolderIcon(int size)
    {
        var path = Path.Combine(AppContext.BaseDirectory, "folder-locked-64.png");
        return File.Exists(path) ? new BitmapImage(new Uri(path)) { DecodePixelWidth = size } : null;
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
