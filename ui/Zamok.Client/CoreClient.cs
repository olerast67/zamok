using System.Buffers;
using System.ComponentModel;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace Zamok.Client;

/// <summary>
/// Клиент канала zamok-core: строка JSON на запрос, строка на ответ.
/// Перед каждым запросом проверяет, что на том конце процесс этого же пользователя:
/// права канала не защищают от процесса, который занял имя раньше core.
/// </summary>
public sealed partial class CoreClient
{
    private readonly string _pipeName;
    private readonly int _connectTimeoutMs;

    public CoreClient(string? pipeName = null, int connectTimeoutMs = 3000)
    {
        _pipeName = StripPrefix(pipeName ?? DefaultPipeName());
        _connectTimeoutMs = connectTimeoutMs;
    }

    /// <summary><c>ZAMOK_PIPE</c> (для тестов) или <c>zamok-&lt;SID пользователя&gt;</c>.</summary>
    public static string DefaultPipeName()
    {
        var env = Environment.GetEnvironmentVariable("ZAMOK_PIPE");
        if (!string.IsNullOrEmpty(env)) return env;
        return $"zamok-{WindowsIdentity.GetCurrent().User!.Value}";
    }

    private static string StripPrefix(string name) =>
        name.StartsWith(@"\\.\pipe\", StringComparison.OrdinalIgnoreCase) ? name[9..] : name;

    public Task<Response> StatusAsync(CancellationToken ct = default) =>
        SendAsync(w => w.WriteString("cmd", "status"), ct);

    public Task<Response> UnlockAsync(string path, string password, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "unlock");
            w.WriteString("path", path);
            w.WriteString("password", password);
        }, ct);

    public Task<Response> UnlockRecoveryAsync(string path, string words, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "unlock_recovery");
            w.WriteString("path", path);
            w.WriteString("words", words);
        }, ct);

    public Task<Response> LockAsync(string path, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "lock");
            w.WriteString("path", path);
        }, ct);

    public Task<Response> EncryptAsync(string path, string password, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "encrypt");
            w.WriteString("path", path);
            w.WriteString("password", password);
        }, ct);

    public Task<Response> JobStatusAsync(string id, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "job_status");
            w.WriteString("id", id);
        }, ct);

    public Task<Response> CancelJobAsync(string id, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "cancel_job");
            w.WriteString("id", id);
        }, ct);

    private async Task<Response> SendAsync(Action<Utf8JsonWriter> body, CancellationToken ct)
    {
        using var pipe = new NamedPipeClientStream(".", _pipeName, PipeDirection.InOut,
            PipeOptions.Asynchronous);
        try
        {
            await pipe.ConnectAsync(_connectTimeoutMs, ct).ConfigureAwait(false);
        }
        catch (Exception e) when (e is TimeoutException or IOException)
        {
            throw new CoreNotRunningException("zamok-core is not running", e);
        }
        VerifyServer(pipe.SafePipeHandle);

        // Запрос пишем в буфер, который потом стираем: в нём бывает пароль
        var buffer = new ArrayBufferWriter<byte>(1024);
        try
        {
            using (var w = new Utf8JsonWriter(buffer))
            {
                w.WriteStartObject();
                body(w);
                w.WriteEndObject();
            }
            await pipe.WriteAsync(buffer.WrittenMemory, ct).ConfigureAwait(false);
            await pipe.WriteAsync("\n"u8.ToArray(), ct).ConfigureAwait(false);
            await pipe.FlushAsync(ct).ConfigureAwait(false);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(
                MemoryMarshal.AsMemory(buffer.WrittenMemory).Span);
        }

        var line = await ReadLineAsync(pipe, ct).ConfigureAwait(false)
            ?? throw new CoreNotRunningException("zamok-core closed the connection");
        return JsonSerializer.Deserialize<Response>(line, Json.Options)
            ?? throw new CoreNotRunningException("empty response");
    }

    private static async Task<string?> ReadLineAsync(Stream s, CancellationToken ct)
    {
        var bytes = new List<byte>(256);
        var one = new byte[4096];
        while (true)
        {
            var n = await s.ReadAsync(one, ct).ConfigureAwait(false);
            if (n == 0) return bytes.Count == 0 ? null : Encoding.UTF8.GetString(bytes.ToArray());
            var nl = Array.IndexOf(one, (byte)'\n', 0, n);
            if (nl >= 0)
            {
                bytes.AddRange(one.AsSpan(0, nl).ToArray());
                return Encoding.UTF8.GetString(bytes.ToArray());
            }
            bytes.AddRange(one.AsSpan(0, n).ToArray());
            if (bytes.Count > 1 << 20) throw new CoreNotRunningException("response too long");
        }
    }

    /// <summary>Владелец процесса-сервера канала совпадает с текущим пользователем.</summary>
    private static void VerifyServer(SafePipeHandle pipe)
    {
        if (!GetNamedPipeServerProcessId(pipe, out var pid))
            throw new CoreNotTrustedException("cannot identify the pipe server");
        var me = WindowsIdentity.GetCurrent().User;
        var owner = ProcessOwner(pid);
        if (me is null || owner is null || !me.Equals(owner))
            throw new CoreNotTrustedException("the pipe server belongs to another user");
    }

    internal static SecurityIdentifier? ProcessOwner(uint pid)
    {
        const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
        const uint TOKEN_QUERY = 0x0008;
        using var process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (process.IsInvalid) return null;
        if (!OpenProcessToken(process, TOKEN_QUERY, out var token)) return null;
        using (token)
        {
            using var identity = new WindowsIdentity(token.DangerousGetHandle());
            return identity.User;
        }
    }

    [LibraryImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint pid);

    [LibraryImport("kernel32.dll", SetLastError = true)]
    private static partial SafeProcessHandle OpenProcess(uint access,
        [MarshalAs(UnmanagedType.Bool)] bool inherit, uint pid);

    [LibraryImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool OpenProcessToken(SafeProcessHandle process, uint access,
        out SafeAccessTokenHandle token);
}
