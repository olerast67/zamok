using System.Text.Json;
using System.Text.Json.Serialization;

namespace Zamok.Client;

/// <summary>Ответ zamok-core (core/crates/zamok-core/src/protocol.rs).</summary>
public sealed record Response(
    bool Ok,
    string? Error,
    FolderInfo[]? Folders,
    FolderInfo? Folder,
    JobInfo? Job,
    JobInfo[]? Jobs,
    string? RecoveryKey,
    string[]? Examples,
    Settings? Settings = null);

public sealed record Settings(bool LockOnScreenLock);

public sealed record FolderInfo(
    string Id,
    string Name,
    string Path,
    string State,
    ulong IdleLockSecs,
    string? Error,
    bool Hello = false)
{
    public bool IsLocked => State == "locked";
    public bool IsUnlocked => State == "unlocked";
}

public sealed record JobInfo(
    string Id,
    string Path,
    string State,
    string Phase,
    ulong FilesDone,
    ulong FilesTotal,
    ulong BytesDone,
    ulong BytesTotal,
    string? Error)
{
    public bool IsRunning => State == "running";
    public bool IsDone => State == "done";

    /// <summary>Доля сделанного по байтам (0..1); пустая папка считается по файлам.</summary>
    public double Fraction => BytesTotal > 0
        ? (double)BytesDone / BytesTotal
        : FilesTotal > 0 ? (double)FilesDone / FilesTotal : (IsRunning ? 0 : 1);
}

internal static class Json
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };
}

/// <summary>zamok-core не запущен или не отвечает.</summary>
public sealed class CoreNotRunningException(string message, Exception? inner = null)
    : Exception(message, inner);

/// <summary>На том конце канала процесс другого пользователя: пароль ему не отправляем.</summary>
public sealed class CoreNotTrustedException(string message) : Exception(message);
