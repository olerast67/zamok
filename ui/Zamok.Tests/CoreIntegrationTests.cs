using System.Diagnostics;
using Zamok.Client;

namespace Zamok.Tests;

/// <summary>
/// Настоящий zamok-core (сборка release) со своей папкой данных и своим каналом.
/// Все папки внутри tests\tmp\ui-client. Нужны WinFsp и собранный core:
/// <c>cargo build --release -p zamok-core</c>. В CI не запускаются (Category=Integration).
/// </summary>
[Trait("Category", "Integration")]
public sealed class CoreIntegrationTests : IDisposable
{
    private static readonly string Repo = FindRepo();
    private readonly string _root = Path.Combine(Repo, "tests", "tmp", "ui-client", Guid.NewGuid().ToString("N")[..8]);
    private readonly string _pipe = $"zamok-ui-test-{Environment.ProcessId}-{Guid.NewGuid():N}";
    private readonly Process _core;
    private readonly CoreClient _client;

    private static string FindRepo()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d is not null && !File.Exists(Path.Combine(d.FullName, "ROADMAP.md"))) d = d.Parent;
        return d?.FullName ?? throw new InvalidOperationException("repository root not found");
    }

    public CoreIntegrationTests()
    {
        var exe = Path.Combine(Repo, "core", "target", "release", "zamok-core.exe");
        Assert.True(File.Exists(exe), "build zamok-core first: cargo build --release -p zamok-core");
        Directory.CreateDirectory(_root);
        var psi = new ProcessStartInfo(exe) { UseShellExecute = false };
        psi.Environment["ZAMOK_DATA_DIR"] = Path.Combine(_root, "data");
        psi.Environment["ZAMOK_NO_TRAY"] = "1";
        psi.Environment["ZAMOK_PIPE"] = $@"\\.\pipe\{_pipe}";
        _core = Process.Start(psi)!;
        var deadline = DateTime.UtcNow.AddSeconds(15);
        while (!File.Exists($@"\\.\pipe\{_pipe}") && DateTime.UtcNow < deadline && !_core.HasExited)
            Thread.Sleep(100);
        _client = new CoreClient(_pipe);
    }

    public void Dispose()
    {
        try { _client.StatusAsync().Wait(2000); } catch { }
        try
        {
            using var p = new System.IO.Pipes.NamedPipeClientStream(".", _pipe);
            p.Connect(2000);
            p.Write("{\"cmd\":\"quit\"}\n"u8);
            p.Flush();
        }
        catch { }
        if (!_core.WaitForExit(15000)) _core.Kill();
        try { Directory.Delete(_root, true); } catch { }
    }

    private string Sample(string name)
    {
        var dir = Path.Combine(_root, name);
        Directory.CreateDirectory(Path.Combine(dir, "Папка"));
        File.WriteAllText(Path.Combine(dir, "Отчёт.txt"), "привет");
        File.WriteAllBytes(Path.Combine(dir, "Папка", "data.bin"), new byte[100_000]);
        return dir;
    }

    private async Task<JobInfo> WaitJob(string id)
    {
        var deadline = DateTime.UtcNow.AddSeconds(60);
        while (true)
        {
            var j = (await _client.JobStatusAsync(id)).Job!;
            if (!j.IsRunning) return j;
            Assert.True(DateTime.UtcNow < deadline, "job still running");
            await Task.Delay(200);
        }
    }

    [Fact]
    public async Task EncryptUnlockLockThroughTheClient()
    {
        Assert.Empty((await _client.StatusAsync()).Folders!);
        var dir = Sample("Клиенты");

        var started = await _client.EncryptAsync(dir, "верный пароль");
        Assert.True(started.Ok, started.Error);
        Assert.Equal(44, RecoveryCheck.Split(started.RecoveryKey!).Length);
        var job = await WaitJob(started.Job!.Id);
        Assert.True(job.IsDone, job.Error);
        Assert.Equal(1.0, job.Fraction);

        var folder = Assert.Single((await _client.StatusAsync()).Folders!);
        Assert.True(folder.IsUnlocked);
        Assert.Equal("привет", File.ReadAllText(Path.Combine(dir, "Отчёт.txt")));

        Assert.True((await _client.LockAsync(dir)).Folder!.IsLocked);
        Assert.False(File.Exists(Path.Combine(dir, "Отчёт.txt")));

        var wrong = await _client.UnlockAsync(dir, "не тот");
        Assert.Equal("wrong_password", wrong.Error);

        var ok = await _client.UnlockAsync(Path.Combine(dir, "Открыть Клиенты.zamok"), "верный пароль");
        Assert.True(ok.Folder!.IsUnlocked);

        await _client.LockAsync(dir);
        var byKey = await _client.UnlockRecoveryAsync(dir, started.RecoveryKey!);
        Assert.True(byKey.Folder!.IsUnlocked);

        // Смена пароля: старый не открывает, новый открывает, ключ восстановления тот же
        Assert.Equal("wrong_password", (await _client.ChangePasswordAsync(dir, "не тот", "новый пароль 1")).Error);
        Assert.True((await _client.ChangePasswordAsync(dir, "верный пароль", "новый пароль 1")).Ok);
        await _client.LockAsync(dir);
        Assert.Equal("wrong_password", (await _client.UnlockAsync(dir, "верный пароль")).Error);
        Assert.True((await _client.UnlockAsync(dir, "новый пароль 1")).Folder!.IsUnlocked);
        Assert.Equal("wrong_password", (await _client.ShowRecoveryKeyAsync(dir, "верный пароль")).Error);
        Assert.Equal(started.RecoveryKey, (await _client.ShowRecoveryKeyAsync(dir, "новый пароль 1")).RecoveryKey);
        Assert.False(File.Exists(Path.Combine(_root, ".Клиенты.zamok", "masterkey.cryptomator.tmp")));

        // Windows Hello на стороне core: ключ обёртки подставной (настоящий даёт подпись Hello)
        var wrap = Convert.ToBase64String(System.Security.Cryptography.RandomNumberGenerator.GetBytes(32));
        var other = Convert.ToBase64String(System.Security.Cryptography.RandomNumberGenerator.GetBytes(32));
        Assert.Equal("wrong_password", (await _client.HelloEnableAsync(dir, "не тот", wrap)).Error);
        Assert.True((await _client.HelloEnableAsync(dir, "новый пароль 1", wrap)).Folder!.Hello);
        await _client.LockAsync(dir);
        Assert.Equal("hello_failed", (await _client.UnlockHelloAsync(dir, other)).Error);
        Assert.True((await _client.UnlockHelloAsync(dir, wrap)).Folder!.IsUnlocked);
        Assert.False((await _client.HelloDisableAsync(dir)).Folder!.Hello);
        await _client.LockAsync(dir);
        Assert.Equal("hello_failed", (await _client.UnlockHelloAsync(dir, wrap)).Error);
    }

    [Fact]
    public async Task OpenFilesAreNamedByProgramAndIdleAnswersWork()
    {
        var dir = Sample("Отчёты");
        var started = await _client.EncryptAsync(dir, "верный пароль");
        Assert.True(started.Ok, started.Error);
        Assert.True((await WaitJob(started.Job!.Id)).IsDone);

        var me = Path.GetFileName(Environment.ProcessPath!);
        using (File.Open(Path.Combine(dir, "Отчёт.txt"), FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
        {
            var f = Assert.Single((await _client.StatusAsync()).Folders!);
            Assert.Equal(1UL, f.OpenFiles);
            // Только имя программы, без пути
            Assert.Equal([me], f.OpenIn!);
            Assert.False(f.IdleAsking);

            // «Отложить» папку не запирает, «Запереть сейчас» запирает при открытом файле
            var later = await _client.AnswerIdleAsync(dir, lockNow: false);
            Assert.True(later.Ok, later.Error);
            Assert.True(later.Folder!.IsUnlocked);
            Assert.True((await _client.AnswerIdleAsync(dir, lockNow: true)).Folder!.IsLocked);
        }
        var locked = Assert.Single((await _client.StatusAsync()).Folders!);
        Assert.Equal(0UL, locked.OpenFiles);
        Assert.Null(locked.OpenIn);
        Assert.Equal("not_found", (await _client.AnswerIdleAsync(Path.Combine(_root, "нет такой"), true)).Error);
    }

    [Fact]
    public async Task RefusalCarriesExamples()
    {
        var dir = Sample("Занято");
        using var held = new FileStream(Path.Combine(dir, "Отчёт.txt"), FileMode.Open, FileAccess.Read, FileShare.Read);
        var r = await _client.EncryptAsync(dir, "пароль-пароль");
        Assert.Equal("files_in_use", r.Error);
        Assert.Equal(["Отчёт.txt"], r.Examples!);
    }
}

public class CoreNotRunningTests
{
    [Fact]
    public async Task MissingCoreIsReportedNotHung()
    {
        var c = new CoreClient($"zamok-nobody-{Guid.NewGuid():N}", connectTimeoutMs: 300);
        await Assert.ThrowsAsync<CoreNotRunningException>(() => c.StatusAsync());
    }
}
