using Zamok.Client;

namespace Zamok.Tests;

/// <summary>
/// Команда для Defender выполняется с правами администратора: путь хранилища не должен
/// попасть в её текст ни в каком виде, иначе кавычка в имени папки выполнила бы код.
/// </summary>
public class DefenderCommandTests
{
    public static TheoryData<string> Tricky() =>
    [
        @"C:\Работа\.Клиенты.zamok",
        @"C:\it's\.a.zamok",
        "C:\\x\u2019; calc; \u2019\\.b.zamok",
        "C:\\x\u2018\\.c.zamok",
        "C:\\x\u201A\\.d.zamok",
        "C:\\x\u201B\\.e.zamok",
        @"C:\$(calc)\.f.zamok",
        "C:\\a`b`$x\\.g.zamok",
        "C:\\\"q\"\\.h.zamok",
    ];

    [Theory]
    [MemberData(nameof(Tricky))]
    public void PathNeverAppearsInTheCommandAndComesBackIntact(string vault)
    {
        var text = DefenderCommand.Text([vault, @"D:\.ok.zamok"]);
        Assert.DoesNotContain(vault, text);
        // Кроме одной пары апострофов вокруг base64, никаких кавычек и подстановок
        Assert.Equal(2, text.Count(c => c == '\''));
        Assert.DoesNotContain('`', text);
        Assert.DoesNotContain('"', text);
        Assert.DoesNotContain("$(", text);
        foreach (var q in "\u2018\u2019\u201A\u201B") Assert.DoesNotContain(q, text);
        Assert.Equal([vault, @"D:\.ok.zamok"], DefenderCommand.PathsIn(text));
    }

    [Theory]
    [MemberData(nameof(Tricky))]
    public void ElevatedArgumentsCarryOnlyEncodedData(string vault)
    {
        var args = DefenderCommand.ElevatedArguments([vault]);
        Assert.StartsWith("-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand ", args);
        var encoded = args[(args.LastIndexOf(' ') + 1)..];
        var script = System.Text.Encoding.Unicode.GetString(Convert.FromBase64String(encoded));
        Assert.StartsWith("$ErrorActionPreference = 'Stop'; Add-MpPreference -ExclusionPath @(ConvertFrom-Json", script);
        Assert.DoesNotContain(vault, script);
        Assert.Equal([vault], DefenderCommand.PathsIn(script));
    }

    [Theory]
    [InlineData("C:\\a\nb\\.x.zamok")]
    [InlineData("C:\\a\rb")]
    [InlineData("C:\\a\u0000b")]
    [InlineData("C:\\a\u001bb")]
    [InlineData("C:\\a\u0085b")]
    [InlineData("")]
    public void ControlCharactersAreRejected(string vault) =>
        Assert.Throws<ArgumentException>(() => DefenderCommand.Text([vault]));
}
