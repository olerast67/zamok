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
