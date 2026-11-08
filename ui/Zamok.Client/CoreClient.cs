using System.Buffers;
using System.ComponentModel;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Encodings.Web;
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

    public Task<Response> ChangePasswordAsync(string path, string oldPassword, string newPassword,
        CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "change_password");
            w.WriteString("path", path);
            w.WriteString("old_password", oldPassword);
            w.WriteString("new_password", newPassword);
        }, ct);

    public Task<Response> ShowRecoveryKeyAsync(string path, string password, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "show_recovery_key");
            w.WriteString("path", path);
            w.WriteString("password", password);
        }, ct);

    public Task<Response> SetIdleLockAsync(string path, ulong secs, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "set_idle_lock");
            w.WriteString("path", path);
            w.WriteNumber("secs", secs);
        }, ct);

    public Task<Response> SetLockOnScreenLockAsync(bool on, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "set_lock_on_screen_lock");
            w.WriteBoolean("on", on);
        }, ct);

    public Task<Response> HelloEnableAsync(string path, string password, string wrapKey, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "hello_enable");
            w.WriteString("path", path);
            w.WriteString("password", password);
            w.WriteString("wrap_key", wrapKey);
        }, ct);

    public Task<Response> HelloDisableAsync(string path, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "hello_disable");
            w.WriteString("path", path);
        }, ct);

    public Task<Response> UnlockHelloAsync(string path, string wrapKey, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "unlock_hello");
            w.WriteString("path", path);
            w.WriteString("wrap_key", wrapKey);
        }, ct);

    public Task<Response> DecryptAsync(string path, string password, CancellationToken ct = default) =>
        SendAsync(w =>
        {
            w.WriteString("cmd", "decrypt");
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

    private async Task<Response> SendOnceAsync(Action<Utf8JsonWriter> body, CancellationToken ct)
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
            using (var w = new Utf8JsonWriter(buffer, new JsonWriterOptions
            {
                // Без экранирования кириллицы: иначе пароль проходит через внутренний буфер
                // разбора serde_json, который core не может стереть
                Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            }))
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
        var trust = Inspect(pid);
        if (trust is null)
            throw new CoreNotTrustedException("cannot inspect the pipe server");
        var me = WindowsIdentity.GetCurrent().User;
        if (me is null || !me.Equals(trust.Owner))
            throw new CoreNotTrustedException("the pipe server belongs to another user");
        // Урезанный процесс этого же пользователя (песочница браузера, низкая целостность)
        // не должен получить пароль, даже если занял имя канала раньше core
        if (trust.IntegrityRid < MediumIntegrityRid || trust.IsAppContainer || trust.IsRestricted)
            throw new CoreNotTrustedException("the pipe server runs with reduced rights");
    }

    internal const uint MediumIntegrityRid = 0x2000;

    /// <summary>Что известно о процессе: владелец и насколько урезаны его права.</summary>
    internal sealed record ProcessTrust(SecurityIdentifier Owner, uint IntegrityRid,
        bool IsAppContainer, bool IsRestricted);

    internal static ProcessTrust? Inspect(uint pid)
    {
        const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
        const uint TOKEN_QUERY = 0x0008;
        using var process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (process.IsInvalid) return null;
        if (!OpenProcessToken(process, TOKEN_QUERY, out var token)) return null;
        using (token)
        {
            using var identity = new WindowsIdentity(token.DangerousGetHandle());
            if (identity.User is null) return null;
            var rid = IntegrityRid(token);
            var appContainer = TokenDword(token, TokenIsAppContainer);
            if (rid is null || appContainer is null) return null;
            return new ProcessTrust(identity.User, rid.Value, appContainer.Value != 0,
                IsTokenRestricted(token));
        }
    }

    private const int TokenIntegrityLevel = 25;
    private const int TokenIsAppContainer = 29;

    /// <summary>Последняя часть SID метки целостности: 0x1000 низкая, 0x2000 средняя, 0x3000 высокая.</summary>
    private static uint? IntegrityRid(SafeAccessTokenHandle token)
    {
        GetTokenInformation(token, TokenIntegrityLevel, IntPtr.Zero, 0, out var len);
        if (len == 0) return null;
        var buf = Marshal.AllocHGlobal((int)len);
        try
        {
            if (!GetTokenInformation(token, TokenIntegrityLevel, buf, len, out _)) return null;
            // TOKEN_MANDATORY_LABEL { SID_AND_ATTRIBUTES { PSID Sid; DWORD Attributes } }
            var sid = new SecurityIdentifier(Marshal.ReadIntPtr(buf));
            var parts = sid.Value.Split('-');
            return uint.Parse(parts[^1]);
        }
        finally
        {
            Marshal.FreeHGlobal(buf);
        }
    }

    private static uint? TokenDword(SafeAccessTokenHandle token, int cls)
    {
        var buf = Marshal.AllocHGlobal(4);
        try
        {
            return GetTokenInformation(token, cls, buf, 4, out _) ? (uint)Marshal.ReadInt32(buf) : null;
        }
        finally
        {
            Marshal.FreeHGlobal(buf);
        }
    }

    [LibraryImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetTokenInformation(SafeAccessTokenHandle token, int cls,
        IntPtr info, uint length, out uint returned);

    [LibraryImport("advapi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool IsTokenRestricted(SafeAccessTokenHandle token);

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
