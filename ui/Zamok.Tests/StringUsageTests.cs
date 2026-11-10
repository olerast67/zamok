using System.Text.RegularExpressions;
using System.Xml.Linq;

namespace Zamok.Tests;

/// <summary>
/// Все видимые строки есть на обоих языках и используются (ROADMAP, этап 6 «Русский и английский»):
/// окна (resw, x:Uid и ключи в коде), установщик (wxs и тексты окна установки). Строки core и
/// DLL Проводника проверяет тест Rust <c>zamok-core/tests/strings.rs</c>.
/// </summary>
public partial class StringUsageTests
{
    private static readonly string Repo = FindRepo();
    private static readonly string Ui = Path.Combine(Repo, "ui", "Zamok.UI");

    private static string FindRepo()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d is not null && !File.Exists(Path.Combine(d.FullName, "ROADMAP.md"))) d = d.Parent;
        return d!.FullName;
    }

    private static Dictionary<string, string> Resw(string lang) =>
        XDocument.Load(Path.Combine(Ui, "Strings", lang, "Resources.resw"))
            .Root!.Elements("data")
            .ToDictionary(e => (string)e.Attribute("name")!, e => (string)e.Element("value")!);

    [GeneratedRegex(@"x:Uid=""([^""]+)""")]
    private static partial Regex XUid();

    /// <summary>Строковые литералы вида Раздел_Имя или Раздел_Имя/Свойство в коде окон (не ZAMOK_LANG).</summary>
    [GeneratedRegex(@"\$?""([A-Z][a-z0-9][A-Za-z0-9]*_[A-Za-z0-9_]+(?:/[A-Za-z]+)?)(\{[a-z]+\})?""")]
    private static partial Regex KeyLiteral();

    [GeneratedRegex(@"[Ѐ-ӿ]")]
    private static partial Regex Cyrillic();

    private static IEnumerable<string> Sources(string pattern) =>
        Directory.GetFiles(Ui, pattern, SearchOption.TopDirectoryOnly);

    private static List<string> Uids() =>
        Sources("*.xaml").SelectMany(f => XUid().Matches(File.ReadAllText(f)).Select(m => m.Groups[1].Value))
            .Distinct().ToList();

    /// <summary>Ключи, которые код читает через Strings.Get и Strings.Format; ключ с подстановкой
    /// (<c>$"Encrypt_Strength{score}"</c>) даёт все ключи с этим началом.</summary>
    private static List<string> CodeKeys(IReadOnlyCollection<string> all)
    {
        var keys = new List<string>();
        foreach (var f in Sources("*.cs"))
        {
            foreach (Match m in KeyLiteral().Matches(File.ReadAllText(f)))
            {
                var key = m.Groups[1].Value.Replace('/', '.');
                if (m.Groups[2].Success) keys.AddRange(all.Where(k => k.StartsWith(key) && k.Length > key.Length));
                else keys.Add(key);
            }
        }
        return keys.Distinct().ToList();
    }

    [Fact]
    public void EveryXUidHasTextInBothLanguages()
    {
        var uids = Uids();
        Assert.True(uids.Count >= 30, $"found only {uids.Count} x:Uid");
        foreach (var lang in new[] { "ru-RU", "en-US" })
        {
            var s = Resw(lang);
            var missing = uids.Where(u => !s.Keys.Any(k => k.StartsWith(u + "."))).ToList();
            Assert.True(missing.Count == 0, $"{lang}: no text for x:Uid {string.Join(", ", missing)}");
        }
    }

    [Fact]
    public void EveryKeyReadInCodeExistsInBothLanguages()
    {
        var ru = Resw("ru-RU");
        var keys = CodeKeys(ru.Keys);
        Assert.Contains("Encrypt_Strength4", keys);
        foreach (var lang in new[] { "ru-RU", "en-US" })
        {
            var s = Resw(lang);
            var missing = keys.Where(k => !s.ContainsKey(k)).ToList();
            Assert.True(missing.Count == 0, $"{lang}: no text for {string.Join(", ", missing)}");
        }
    }

    [Fact]
    public void EveryStringIsUsed()
    {
        var ru = Resw("ru-RU");
        var uids = Uids();
        var code = CodeKeys(ru.Keys);
        // Тексты ошибок выбираются по коду ответа core (ResourceTests сверяет их с protocol.rs)
        var unused = ru.Keys.Where(k => !k.StartsWith("Error_") && !code.Contains(k) &&
                                        !uids.Contains(k.Split('.')[0])).ToList();
        Assert.True(unused.Count == 0, $"strings nobody shows: {string.Join(", ", unused)}");
    }

    [Fact]
    public void WindowsHaveNoTextOutsideTheStringFiles()
    {
        foreach (var f in Sources("*.xaml"))
        {
            var xaml = Regex.Replace(File.ReadAllText(f), "<!--.*?-->", "", RegexOptions.Singleline);
            Assert.False(Cyrillic().IsMatch(xaml), $"{Path.GetFileName(f)}: Russian text in the markup");
            foreach (Match m in Regex.Matches(xaml, @"\b(Text|Content|Header|PlaceholderText|ToolTipService\.ToolTip|AutomationProperties\.Name)=""([^""{][^""]*)"""))
                Assert.True(m.Groups[2].Value == "Zamok", $"{Path.GetFileName(f)}: fixed text {m.Value}");
        }
        foreach (var f in Sources("*.cs"))
        {
            var code = Regex.Replace(File.ReadAllText(f), @"//.*", "");
            Assert.False(Cyrillic().IsMatch(code), $"{Path.GetFileName(f)}: Russian text in the code");
        }
    }

    [Fact]
    public void InstallerWindowHasTheSameTextsInBothLanguages()
    {
        static Dictionary<string, string> Wxl(string name) =>
            XDocument.Load(Path.Combine(Repo, "installer", name)).Root!.Elements()
                .ToDictionary(e => (string)e.Attribute("Id")!, e => (string)e.Attribute("Value")!);
        var ru = Wxl("thm-ru.wxl");
        var en = Wxl("thm-en.wxl");
        Assert.Equal(en.Keys.Order(), ru.Keys.Order());
        Assert.All(ru.Concat(en), kv => Assert.False(string.IsNullOrWhiteSpace(kv.Value), kv.Key));
        Assert.All(ru.Where(kv => !kv.Key.StartsWith("Help")), kv =>
            Assert.True(Cyrillic().IsMatch(kv.Value) || !Regex.IsMatch(kv.Value, "[A-Za-z]{4}") || kv.Value.Contains("[Wix"), $"thm-ru.wxl {kv.Key} is not Russian"));
        Assert.All(en, kv => Assert.False(Cyrillic().IsMatch(kv.Value), $"thm-en.wxl {kv.Key} is not English"));
        // Строки темы, которые показывает окно, есть в файлах перевода
        var theme = File.ReadAllText(Path.Combine(Repo, "installer", "thm.xml"));
        var used = Regex.Matches(theme, @"#\(loc\.([A-Za-z]+)\)").Select(m => m.Groups[1].Value).Distinct();
        Assert.All(used, id => Assert.True(en.ContainsKey(id), $"thm.xml uses {id}, no text for it"));
    }

    [Fact]
    public void MsiTextsAreEnglishWithRussianForRussianWindows()
    {
        var wxs = XDocument.Load(Path.Combine(Repo, "installer", "Zamok.wxs"));
        XNamespace ns = "http://wixtoolset.org/schemas/v4/wxs";
        var props = wxs.Descendants(ns + "Property").Where(p => ((string)p.Attribute("Id")!).StartsWith("T_"))
            .ToDictionary(p => (string)p.Attribute("Id")!, p => (string)p.Attribute("Value")!);
        var russian = wxs.Descendants(ns + "SetProperty")
            .Where(p => ((string?)p.Attribute("Condition"))?.Contains("UserLanguageID = 1049") == true)
            .ToDictionary(p => (string)p.Attribute("Id")!, p => (string)p.Attribute("Value")!);
        Assert.True(props.Count >= 7, $"found only {props.Count} texts");
        Assert.Equal(props.Keys.Order(), russian.Keys.Order());
        Assert.All(props, kv => Assert.False(Cyrillic().IsMatch(kv.Value), $"{kv.Key} default is not English"));
        Assert.All(russian, kv => Assert.True(Cyrillic().IsMatch(kv.Value), $"{kv.Key} for Russian Windows is not Russian"));
        // Русский текст только в SetProperty для русской Windows, остальное через [T_...]
        foreach (var a in wxs.Descendants().Where(e => e.Name != ns + "SetProperty").SelectMany(e => e.Attributes()))
            Assert.False(Cyrillic().IsMatch(a.Value), $"{a.Parent!.Name.LocalName} {a.Name}: Russian only");
        var usedProps = Regex.Matches(wxs.ToString(), @"\[(T_[A-Z]+)\]").Select(m => m.Groups[1].Value).Distinct().ToList();
        Assert.Equal(props.Keys.Order(), usedProps.Order());
    }
}
