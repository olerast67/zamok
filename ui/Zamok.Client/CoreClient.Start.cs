using System.Diagnostics;
using System.Text.Json;

namespace Zamok.Client;

/// <summary>
/// Обёртка над запросом: сбой канала становится понятной окну ошибкой, а если core не запущен,
/// он запускается один раз за жизнь окна и запрос повторяется.
/// </summary>
public sealed partial class CoreClient
{
    private static int _startTried;

    private async Task<Response> SendAsync(Action<Utf8JsonWriter> body, CancellationToken ct)
    {
        try
        {
            return await SendConvertedAsync(body, ct).ConfigureAwait(false);
        }
        catch (CoreNotRunningException e) when (e.InnerException is not null)
        {
            // InnerException есть только у сбоя подключения: запрос не ушёл, повтор безопасен.
            // После обрыва на середине запрос не повторяем, core мог его уже выполнить
            if (!TryStartCore()) throw;
            await WaitForPipeAsync(ct).ConfigureAwait(false);
            return await SendConvertedAsync(body, ct).ConfigureAwait(false);
        }
    }

    private async Task<Response> SendConvertedAsync(Action<Utf8JsonWriter> body, CancellationToken ct)
    {
        try
        {
            return await SendOnceAsync(body, ct).ConfigureAwait(false);
        }
        catch (Exception e) when (e is IOException or JsonException or ObjectDisposedException ||
                                  (e is OperationCanceledException && !ct.IsCancellationRequested))
        {
            throw new CoreNotRunningException("zamok-core stopped answering");
        }
    }

    /// <summary>Запустить установленный zamok-core --watch рядом с Zamok.exe. Только для настоящего канала.</summary>
    private bool TryStartCore()
    {
        if (!string.IsNullOrEmpty(Environment.GetEnvironmentVariable("ZAMOK_PIPE"))) return false;
        if (!string.Equals(_pipeName, StripPrefix(DefaultPipeName()), StringComparison.OrdinalIgnoreCase)) return false;
        if (Interlocked.Exchange(ref _startTried, 1) != 0) return false;
        var exe = Path.Combine(AppContext.BaseDirectory, "zamok-core.exe");
        if (!File.Exists(exe)) return false;
        try
        {
            using var p = Process.Start(new ProcessStartInfo(exe, "--watch")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            });
            return p is not null;
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException)
        {
            return false;
        }
    }

    private async Task WaitForPipeAsync(CancellationToken ct)
    {
        var path = @"\\.\pipe\" + _pipeName;
        for (var i = 0; i < 50 && !File.Exists(path); i++)
            await Task.Delay(200, ct).ConfigureAwait(false);
    }
}
