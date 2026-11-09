using System.Runtime.InteropServices;

namespace Zamok.UI;

/// <summary>
/// Окно Windows Hello открывает отдельная системная программа, и у обычного приложения оно
/// появляется позади его окон. Пока идёт запрос, ищем это окно по классу и выводим его наверх.
/// </summary>
internal sealed class HelloFocus : IDisposable
{
    private const string DialogClass = "Credential Dialog Xaml Host";
    private const uint SwpNoMove = 0x0002, SwpNoSize = 0x0001, SwpShowWindow = 0x0040;
    private static readonly IntPtr Topmost = new(-1);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr FindWindowW(string cls, string? title);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    private readonly CancellationTokenSource _stop = new();

    public HelloFocus() => _ = Task.Run(WatchAsync);

    private async Task WatchAsync()
    {
        var last = IntPtr.Zero;
        try
        {
            while (!_stop.IsCancellationRequested)
            {
                var h = FindWindowW(DialogClass, null);
                // Запросов бывает два подряд (создание ключа и подпись): новое окно снова наверх
                if (h != IntPtr.Zero && h != last)
                {
                    last = h;
                    if (!SetForegroundWindow(h))
                        SetWindowPos(h, Topmost, 0, 0, 0, 0, SwpNoMove | SwpNoSize | SwpShowWindow);
                }
                await Task.Delay(100, _stop.Token);
            }
        }
        catch (OperationCanceledException) { }
    }

    public void Dispose() => _stop.Cancel();
}
