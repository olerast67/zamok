using Zamok.Client;

namespace Zamok.Tests;

public class PasswordStrengthTests
{
    [Theory]
    [InlineData("")]
    [InlineData("Ab1!")]
    [InlineData("1234567")]
    public void ShorterThanEightIsZero(string pw) => Assert.Equal(0, PasswordStrength.Score(pw));

    [Theory]
    [InlineData("aaaaaaaaaaaa")]
    [InlineData("12345678")]
    [InlineData("qwertyuiop")]
    [InlineData("password1")]
    [InlineData("йцукенгшщз")]
    public void PatternsAndCommonPasswordsAreWeak(string pw) =>
        Assert.Equal(1, PasswordStrength.Score(pw));

    [Theory]
    [InlineData("Корова летит на луну 2026")]
    [InlineData("correct horse battery staple")]
    [InlineData("T7#qL9!vZp2@mX4w")]
    public void LongOrVariedPasswordsAreStrong(string pw) =>
        Assert.Equal(4, PasswordStrength.Score(pw));

    [Fact]
    public void LongerIsNeverWeaker()
    {
        var pw = "Zk4!";
        var last = 0;
        for (var i = 0; i < 6; i++)
        {
            pw += "mR8$";
            var s = PasswordStrength.Score(pw);
            Assert.True(s >= last, $"{pw}: {s} < {last}");
            last = s;
        }
    }
}

public class RecoveryCheckTests
{
    private const string Key = "one two three four five six seven eight nine ten";

    [Fact]
    public void PicksFourDistinctPositionsInRange()
    {
        for (var i = 0; i < 200; i++)
        {
            var c = new RecoveryCheck(Key);
            Assert.Equal(4, c.Positions.Count);
            Assert.Equal(c.Positions.Distinct().Count(), c.Positions.Count);
            Assert.All(c.Positions, p => Assert.InRange(p, 1, 10));
            Assert.Equal(c.Positions.OrderBy(p => p), c.Positions);
        }
    }

    [Fact]
    public void AcceptsCaseAndSpacesRejectsWrongWords()
    {
        var c = new RecoveryCheck(Key, [1, 3, 5, 10]);
        Assert.True(c.AllCorrect(["One", " three ", "FIVE", "ten"]));
        Assert.False(c.AllCorrect(["one", "three", "five", "nine"]));
        Assert.False(c.AllCorrect(["one", "three", "five"]));
        Assert.False(c.IsCorrect(1, null));
    }
}

public class ProcessTrustTests
{
    [Fact]
    public void CurrentProcessIsTrustedAtMediumOrHigherWithoutSandbox()
    {
        var t = CoreClient.Inspect((uint)Environment.ProcessId)!;
        Assert.NotNull(t);
        Assert.Equal(System.Security.Principal.WindowsIdentity.GetCurrent().User, t.Owner);
        Assert.True(t.IntegrityRid >= CoreClient.MediumIntegrityRid, $"rid {t.IntegrityRid:x}");
        Assert.False(t.IsAppContainer);
        Assert.False(t.IsRestricted);
    }

    [Fact]
    public void UnknownProcessCannotBeInspected() =>
        Assert.Null(CoreClient.Inspect(0xFFFFFFF0));
}

public class RecoveryFormatTests
{
    private static readonly string Key = string.Join(' ', Enumerable.Range(1, 44).Select(i => $"w{(char)('a' + i % 26)}{i}"));

    [Fact]
    public void TableHasElevenNumberedRowsOfFour()
    {
        var lines = RecoveryCheck.FormatTable(Key).Split("\r\n");
        Assert.Equal(11, lines.Length);
        Assert.StartsWith(" 1. ", lines[0]);
        Assert.Contains("44. ", lines[10]);
        Assert.All(lines, l => Assert.Equal(4, l.Split('.').Length - 1));
    }

    [Fact]
    public void NormalizeReadsTheWholeSavedFile()
    {
        var file = "Recovery key Zamok for the folder “Clients”\r\nSaved 2026-11-08 12:00\r\n\r\n"
                   + RecoveryCheck.FormatTable(Key)
                   + "\r\n\r\nHow to open: double click “Open Clients.zamok” and paste this file.";
        Assert.Equal(Key, RecoveryCheck.Normalize(file));
    }

    [Theory]
    [InlineData("  Flower   casualty\n sometimes ", "flower casualty sometimes")]
    [InlineData("3. c 1. a 2. b", "a b c")]
    public void NormalizePlainWordsAndOrder(string input, string expected) =>
        Assert.Equal(expected, RecoveryCheck.Normalize(input));
}
