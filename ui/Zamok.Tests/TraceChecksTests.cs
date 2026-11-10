using Zamok.Client;
using static Zamok.Client.TraceChecks;

namespace Zamok.Tests;

/// <summary>Решения «Проверки следов» (DESIGN.md, раздел 6) по прочитанным значениям.</summary>
public class TraceChecksTests
{
    [Theory]
    [InlineData(1, Disk.On)]
    [InlineData(6, Disk.On)]
    [InlineData(2, Disk.Off)]
    [InlineData(4, Disk.Off)]
    [InlineData(3, Disk.Encrypting)]
    [InlineData(5, Disk.Paused)]
    [InlineData(8, Disk.Waiting)]
    [InlineData(0, Disk.Unknown)]
    [InlineData(null, Disk.Unknown)]
    public void BitLockerValues(int? value, Disk want) => Assert.Equal(want, DiskFromBitLocker(value));

    [Fact]
    public void OnlyFullProtectionOfTheDiskIsGood()
    {
        Assert.Equal(TraceState.Good, StateOf(Disk.On));
        // Приостановлено и «ждёт включения»: ключ на диске открыто, это не защита
        Assert.Equal(TraceState.Warning, StateOf(Disk.Paused));
        Assert.Equal(TraceState.Warning, StateOf(Disk.Waiting));
        Assert.Equal(TraceState.Unknown, StateOf(Disk.Unknown));
    }

    [Theory]
    [InlineData(true, null, Disk.Off, Hiber.On)]
    [InlineData(true, 0, Disk.Off, Hiber.On)]
    [InlineData(true, 1, Disk.On, Hiber.OnSafe)]
    [InlineData(false, null, Disk.Off, Hiber.Off)]
    [InlineData(false, 1, Disk.Off, Hiber.On)]
    [InlineData(null, 0, Disk.Off, Hiber.Off)]
    [InlineData(null, null, Disk.Off, Hiber.Unknown)]
    public void Hibernation(bool? file, int? enabled, Disk disk, Hiber want) =>
        Assert.Equal(want, TraceChecks.Hibernation(file, enabled, disk));

    [Fact]
    public void PageFile()
    {
        Assert.Equal(TraceChecks.PageFile.Encrypted, PageFileState(1, null, "C:", Disk.Off));
        Assert.Equal(TraceChecks.PageFile.Plain, PageFileState(0, ["C:\\pagefile.sys 0 0"], "C:", Disk.Off));
        Assert.Equal(TraceChecks.PageFile.OnEncryptedDisk, PageFileState(0, ["?:\\pagefile.sys"], "C:", Disk.On));
        Assert.Equal(TraceChecks.PageFile.OnEncryptedDisk, PageFileState(null, ["c:\\pagefile.sys 0 0", ""], "C:", Disk.On));
        // Подкачка и на другом диске: шифрование системного диска её не закрывает
        Assert.Equal(TraceChecks.PageFile.Plain, PageFileState(0, ["C:\\pagefile.sys", "D:\\pagefile.sys"], "C:", Disk.On));
        // Без прав значение не прочитать: не знаем, а не «не шифруется»
        Assert.Equal(TraceChecks.PageFile.Unknown, PageFileState(null, null, "C:", Disk.Off));
    }

    [Fact]
    public void Index()
    {
        Assert.Equal(TraceChecks.Index.Unknown, IndexState(0, coreAnswered: false, null, false));
        Assert.Equal(TraceChecks.Index.NoFolders, IndexState(0, true, null, false));
        Assert.Equal(TraceChecks.Index.SearchOff, IndexState(2, true, null, false));
        Assert.Equal(TraceChecks.Index.Outside, IndexState(2, true, [], true));
        Assert.Equal(TraceChecks.Index.Inside, IndexState(2, true, ["C:\\a"], true));
        Assert.Equal(TraceState.Warning, StateOf(TraceChecks.Index.Inside));
    }

    [Fact]
    public void Thumbnails()
    {
        Assert.Equal(TraceChecks.Thumbs.IconsOnly, ThumbsState(1));
        Assert.Equal(TraceChecks.Thumbs.Cached, ThumbsState(0));
        Assert.Equal(TraceChecks.Thumbs.Cached, ThumbsState(null));
    }

    [Fact]
    public void FolderUrlIsTheSameAsInCore() =>
        Assert.Equal("file:///C:\\a\\b\\", FolderUrl("C:\\a\\b\\"));
}

/// <summary>
/// Чтение на настоящей Windows без прав администратора: свойство оболочки, мощности питания,
/// область индексирования. Ничего не меняет. В CI не запускается (Category=Integration).
/// </summary>
[Trait("Category", "Integration")]
public class TraceProbeTests
{
    [Fact]
    public void SystemDriveBitLockerStateIsReadWithoutAdmin()
    {
        var drive = (Environment.GetEnvironmentVariable("SystemDrive") ?? "C:") + "\\";
        Assert.NotNull(TraceProbe.BitLocker(drive));
    }

    [Fact]
    public void PowerCapabilitiesAreRead() => Assert.NotNull(TraceProbe.HiberFilePresent());

    [Fact]
    public void ProfileIsInTheDefaultIndexScope()
    {
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var found = TraceProbe.InCrawlScope([profile]);
        // Служба поиска выключена: проверять нечего
        if (found is null) return;
        Assert.Contains(profile, found);
    }

    [Fact]
    public void RunDoesNotThrowWithoutCore()
    {
        var r = TraceProbe.Run(null);
        Assert.Equal(TraceChecks.Index.Unknown, r.Index);
    }
}
