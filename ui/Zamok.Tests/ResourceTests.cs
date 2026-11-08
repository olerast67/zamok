using System.Text.RegularExpressions;
using System.Xml.Linq;

namespace Zamok.Tests;

/// <summary>
/// Каждый код ошибки core (enum ErrorCode в protocol.rs) имеет текст на обоих языках,
/// и оба файла строк содержат одинаковые ключи. Иначе окно показало бы ключ вместо текста.
/// </summary>
public partial class ResourceTests
{
    private static readonly string Repo = FindRepo();

    private static string FindRepo()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d is not null && !File.Exists(Path.Combine(d.FullName, "ROADMAP.md"))) d = d.Parent;
        return d!.FullName;
    }

    private static Dictionary<string, string> Resw(string lang) =>
        XDocument.Load(Path.Combine(Repo, "ui", "Zamok.UI", "Strings", lang, "Resources.resw"))
            .Root!.Elements("data")
            .ToDictionary(e => (string)e.Attribute("name")!, e => (string)e.Element("value")!);

    [GeneratedRegex(@"(?<!^)([A-Z])")]
    private static partial Regex Camel();

    /// <summary>Варианты enum ErrorCode из protocol.rs в snake_case, как их пишет serde:
    /// подчёркивание перед каждой заглавной буквой, кроме первой.</summary>
    private static List<string> CoreErrorCodes()
    {
        var src = File.ReadAllText(Path.Combine(Repo, "core", "crates", "zamok-core", "src", "protocol.rs"));
        var body = Regex.Match(src, @"pub enum ErrorCode \{(.*?)\n\}", RegexOptions.Singleline).Groups[1].Value;
        return Regex.Matches(body, @"^\s{4}([A-Z][A-Za-z]+),", RegexOptions.Multiline)
            .Select(m => Camel().Replace(m.Groups[1].Value, "_$1").ToLowerInvariant())
            .ToList();
    }

    [Fact]
    public void EveryCoreErrorHasTextInBothLanguages()
    {
        var codes = CoreErrorCodes();
        Assert.Contains("wrong_password", codes);
        Assert.Contains("recovery_pending", codes);
        Assert.True(codes.Count >= 20, $"parsed only {codes.Count} codes");
        foreach (var lang in new[] { "ru-RU", "en-US" })
        {
            var s = Resw(lang);
            var missing = codes.Where(c => !s.ContainsKey("Error_" + c)).ToList();
            Assert.True(missing.Count == 0, $"{lang}: no text for {string.Join(", ", missing)}");
        }
    }

    [Fact]
    public void BothLanguagesHaveTheSameKeysAndNoEmptyTexts()
    {
        var ru = Resw("ru-RU");
        var en = Resw("en-US");
        Assert.Equal(ru.Keys.Order(), en.Keys.Order());
        Assert.All(ru.Concat(en), kv => Assert.False(string.IsNullOrWhiteSpace(kv.Value), kv.Key));
    }

    [Fact]
    public void FormatPlaceholdersMatchBetweenLanguages()
    {
        var ru = Resw("ru-RU");
        var en = Resw("en-US");
        foreach (var key in ru.Keys)
        {
            string[] Holes(string s) => [.. Regex.Matches(s, @"\{(\d+)").Select(m => m.Groups[1].Value).Distinct().Order()];
            Assert.Equal(Holes(ru[key]), Holes(en[key]));
        }
    }
}
