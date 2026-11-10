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

    /// <summary>
    /// Подогнать высоту окна под содержимое при заданной ширине (логические точки). Меряется
    /// сама разметка, поэтому при любом масштабе и шрифте ничего не обрезается.
    /// </summary>
    public static void FitHeight(Window window, FrameworkElement root, int width)
    {
        root.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            // Окно не выше рабочей области экрана: сначала шире (до 1,6 исходной ширины и
            // не шире экрана), потом высота обрезается, и прокрутку берёт ScrollViewer внутри
            var (maxW, maxH) = WorkAreaLogical(window);
            maxH -= 32;
            var cap = Math.Min((int)(width * 1.6), maxW - 48);
            int Measure(int w)
            {
                root.Measure(new Windows.Foundation.Size(w, double.PositiveInfinity));
                return (int)Math.Ceiling(root.DesiredSize.Height);
            }
            var height = Measure(width);
            while (height > maxH && width + 40 <= cap)
            {
                width += 40;
                height = Measure(width);
            }
            ResizeClient(window, width, Math.Clamp(height, 160, Math.Max(160, maxH)));
        });
    }

    /// <summary>Рабочая область экрана окна (без панели задач) в логических точках.</summary>
    private static (int Width, int Height) WorkAreaLogical(Window window)
    {
        var hwnd = Handle(window);
        var scale = GetDpiForWindow(hwnd) / 96.0;
        var area = DisplayArea.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd),
            DisplayAreaFallback.Primary).WorkArea;
        return ((int)(area.Width / scale), (int)(area.Height / scale));
    }

    /// <summary>Размер клиентской области (логические точки), окно по центру экрана.</summary>
    public static void ResizeClient(Window window, int width, int height)
    {
        var hwnd = Handle(window);
        var scale = GetDpiForWindow(hwnd) / 96.0;
        var app = window.AppWindow;
        app.ResizeClient(new SizeInt32((int)Math.Ceiling(width * scale), (int)Math.Ceiling(height * scale)));
        var area = DisplayArea.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd),
            DisplayAreaFallback.Primary).WorkArea;
        app.Move(new PointInt32(area.X + (area.Width - app.Size.Width) / 2,
            area.Y + (area.Height - app.Size.Height) / 2));
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

    /// <summary>Значок программы (синяя плитка с замком, assets/make-icons.py).</summary>
    public static string? AppIconPath() =>
        new[] { "zamok.ico" }
            .Select(n => Path.Combine(AppContext.BaseDirectory, n)).FirstOrDefault(File.Exists);

    /// <summary>Значок запертой папки Zamok (системная папка с замком), если он собран.</summary>
    public static ImageSource? FolderIcon(int size)
    {
        var path = Path.Combine(AppContext.BaseDirectory, "folder-locked-64.png");
        return File.Exists(path) ? new BitmapImage(new Uri(path)) { DecodePixelWidth = size } : null;
    }

    public static IntPtr Handle(Window window) => WinRT.Interop.WindowNative.GetWindowHandle(window);

    /// <summary>
    /// Enter и Esc как в системном диалоге. Обработчик на всём окне и до самого элемента, поэтому
    /// работает и в поле с несколькими строками. Enter на кнопке нажимает её саму, Shift+Enter
    /// в многострочном поле переносит строку.
    /// </summary>
    public static void HookKeys(FrameworkElement root, Action? enter, Action? escape)
    {
        root.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Escape && escape is not null)
            {
                e.Handled = true;
                escape();
            }
            else if (e.Key == Windows.System.VirtualKey.Enter && enter is not null &&
                     e.OriginalSource is not (Microsoft.UI.Xaml.Controls.Primitives.ButtonBase or ComboBox or ToggleSwitch or ListViewItem) &&
                     !(e.OriginalSource is TextBox { AcceptsReturn: true } && IsShiftDown()))
            {
                e.Handled = true;
                enter();
            }
        };
    }

    private static bool IsShiftDown() =>
        Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(Windows.System.VirtualKey.Shift)
            .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

    /// <summary>
    /// Показать текст в строке с <c>AutomationProperties.LiveSetting</c> (ошибка, «Сохранено»):
    /// пустой текст прячет строку. TextBlock сам не сообщает о новом тексте, поэтому событие
    /// живой области поднимается здесь, и экранный диктор читает текст без перехода к нему.
    /// </summary>
    public static void Announce(TextBlock block, string text)
    {
        block.Text = text;
        block.Visibility = string.IsNullOrEmpty(text) ? Visibility.Collapsed : Visibility.Visible;
        if (string.IsNullOrEmpty(text)) return;
        var peer = Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer.FromElement(block)
                   ?? Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer.CreatePeerForElement(block);
        peer?.RaiseAutomationEvent(Microsoft.UI.Xaml.Automation.Peers.AutomationEvents.LiveRegionChanged);
    }

    /// <summary>
    /// Положить в буфер обмена. Пока буфер держит другая программа, Windows отказывает
    /// (CLIPBRD_E_CANT_OPEN): несколько попыток в течение секунды, потом false.
    /// </summary>
    public static async Task<bool> ClipboardAsync(Action put)
    {
        for (var i = 0; ; i++)
        {
            try
            {
                put();
                return true;
            }
            catch (System.Runtime.InteropServices.COMException) when (i < 9)
            {
                await Task.Delay(100);
            }
            catch (System.Runtime.InteropServices.COMException)
            {
                return false;
            }
        }
    }

    /// <summary>Нажать кнопку, если её сейчас можно нажать.</summary>
    public static bool CanPress(Control button) => button.IsEnabled && button.Visibility == Visibility.Visible;

    private const uint MbOk = 0, MbOkCancel = 1, MbIconWarning = 0x30, MbIconError = 0x10,
        MbDefButton2 = 0x100, MbSetForeground = 0x10000, MbTopmost = 0x40000;

    /// <summary>Системное окно с вопросом «ОК / Отмена»; Отмена выбрана заранее.</summary>
    public static bool Confirm(string text, IntPtr owner = 0) =>
        MessageBoxW(owner, text, "Zamok", MbOkCancel | MbIconWarning | MbDefButton2 | MbSetForeground | MbTopmost) == 1;

    /// <summary>Системное окно с ошибкой: годится и когда своего окна нет (<c>--lock</c>).</summary>
    public static void Error(string text, IntPtr owner = 0) =>
        MessageBoxW(owner, text, "Zamok", MbOk | MbIconError | MbSetForeground | MbTopmost);

    [LibraryImport("user32.dll")]
    private static partial uint GetDpiForWindow(IntPtr hwnd);

    [LibraryImport("user32.dll", StringMarshalling = StringMarshalling.Utf16)]
    private static partial int MessageBoxW(IntPtr hwnd, string text, string caption, uint type);
}

/// <summary>
/// Язык окон: русский, если язык интерфейса Windows русский, иначе английский (то же правило
/// в core, <c>howto::user_is_russian</c>). <c>ZAMOK_LANG=ru</c> или <c>en</c> задаёт язык для
/// тестов и снимков окон.
/// </summary>
internal static partial class Lang
{
    public static bool Russian { get; } = Detect();

    public static string Code => Russian ? "ru-RU" : "en-US";

    private static bool Detect()
    {
        var forced = Environment.GetEnvironmentVariable("ZAMOK_LANG");
        if (forced is "ru" or "en") return forced == "ru";
        return (GetUserDefaultUILanguage() & 0x3FF) == 0x19;
    }

    [LibraryImport("kernel32.dll")]
    private static partial ushort GetUserDefaultUILanguage();
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
