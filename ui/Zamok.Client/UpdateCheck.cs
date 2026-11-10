using System.Net;
using System.Reflection;
using System.Text.Json;

namespace Zamok.Client;

/// <summary>Чем закончилась проверка обновлений.</summary>
public enum UpdateState
{
    /// <summary>Новее запущенной версии на GitHub ничего нет.</summary>
    UpToDate,
    /// <summary>Вышла версия новее: <see cref="UpdateResult.Version"/> и страница <see cref="UpdateResult.Url"/>.</summary>
    NewVersion,
    /// <summary>У проекта на GitHub ещё нет ни одного выпуска.</summary>
    NoReleases,
    /// <summary>Нет связи, GitHub ответил ошибкой или непонятным текстом.</summary>
    Failed,
}

public sealed record UpdateResult(UpdateState State, string? Version = null, string? Url = null);

/// <summary>Выпуск из ответа GitHub: тег и страница.</summary>
internal sealed record ReleaseInfo(string Tag, string? HtmlUrl, bool Prerelease);

/// <summary>
/// Проверка обновлений по кнопке в настройках (DESIGN.md, раздел 11): один запрос HTTPS к API
/// GitHub, только когда человек нажал кнопку. Без cookie и без идентификаторов, в запросе только
/// User-Agent с версией. Ничего не скачивает и не ставит: показывает, есть ли версия новее,
/// и ссылку на её страницу. <c>/releases/latest</c> не отдаёт предварительные выпуски, поэтому
/// при ответе 404 берётся список последних выпусков и первый не черновик.
/// </summary>
public sealed class UpdateChecker
{
    public const string GitHubBase = "https://api.github.com/repos/olerast67/zamok";
    public const string ReleasesPage = "https://github.com/olerast67/zamok/releases";
    public static readonly TimeSpan Timeout = TimeSpan.FromSeconds(10);
    /// <summary>Ответ больше этого не нужен ни при каком числе выпусков: дальше не читаем.</summary>
    private const int MaxResponseBytes = 2 * 1024 * 1024;

    private readonly HttpClient _http;
    private readonly string _current;
    private readonly string _base;

    /// <param name="http">Клиент HTTP; в тестах с подменённым обработчиком.</param>
    /// <param name="currentVersion">Версия запущенной программы, например <c>0.1.0-alpha</c>.</param>
    /// <param name="apiBase">Адрес репозитория в API; null: GitHub.</param>
    public UpdateChecker(HttpClient http, string currentVersion, string? apiBase = null)
    {
        _http = http;
        _current = currentVersion;
        _base = (apiBase ?? GitHubBase).TrimEnd('/');
    }

    /// <summary>Клиент для настоящей проверки: без cookie, без учётных данных Windows, 10 секунд.</summary>
    public static HttpClient CreateHttpClient() => new(CreateHandler())
    {
        Timeout = Timeout,
        MaxResponseContentBufferSize = MaxResponseBytes,
    };

    internal static HttpClientHandler CreateHandler() => new()
    {
        UseCookies = false,
        UseDefaultCredentials = false,
        AllowAutoRedirect = true,
        MaxAutomaticRedirections = 3,
    };

    /// <summary>
    /// Адрес для тестов из <c>ZAMOK_UPDATE_URL</c>. Принимается только адрес на этом компьютере
    /// (127.0.0.1, ::1, localhost): переменная окружения не может увести запрос на чужой сервер.
    /// </summary>
    public static string? BaseFromEnvironment() => LoopbackOnly(Environment.GetEnvironmentVariable("ZAMOK_UPDATE_URL"));

    internal static string? LoopbackOnly(string? url)
    {
        if (string.IsNullOrWhiteSpace(url)) return null;
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u)) return null;
        if (u.Scheme != Uri.UriSchemeHttp && u.Scheme != Uri.UriSchemeHttps) return null;
        return u.IsLoopback ? url : null;
    }

    /// <summary>Версия запущенной программы из сборки: <c>InformationalVersion</c> без <c>+коммит</c>.</summary>
    public static string RunningVersion(Assembly? assembly)
    {
        var info = assembly?.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;
        if (string.IsNullOrEmpty(info)) return assembly?.GetName().Version?.ToString(3) ?? "0.0.0";
        var plus = info.IndexOf('+');
        return plus >= 0 ? info[..plus] : info;
    }

    public async Task<UpdateResult> CheckAsync(CancellationToken ct = default)
    {
        try
        {
            ReleaseInfo? release;
            using (var r = await GetAsync("/releases/latest", ct))
            {
                if (r.StatusCode == HttpStatusCode.NotFound)
                {
                    release = null;
                }
                else
                {
                    if (!r.IsSuccessStatusCode) return new(UpdateState.Failed);
                    release = ParseRelease(await r.Content.ReadAsStringAsync(ct));
                    if (release is null) return new(UpdateState.Failed);
                }
            }
            if (release is null)
            {
                // Только предварительные выпуски или ни одного: список последних
                using var r = await GetAsync("/releases?per_page=5", ct);
                if (!r.IsSuccessStatusCode) return new(UpdateState.Failed);
                var list = ParseList(await r.Content.ReadAsStringAsync(ct));
                if (list is null) return new(UpdateState.Failed);
                if (list.Count == 0) return new(UpdateState.NoReleases);
                release = list[0];
            }
            return Decide(release, _current);
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException or IOException
                                      or InvalidOperationException)
        {
            // Нет сети, имя не нашлось, сертификат не прошёл, 10 секунд вышли, ответ слишком большой
            return new(UpdateState.Failed);
        }
    }

    private Task<HttpResponseMessage> GetAsync(string path, CancellationToken ct)
    {
        var req = new HttpRequestMessage(HttpMethod.Get, _base + path);
        req.Headers.UserAgent.ParseAdd("Zamok/" + SafeVersion(_current));
        req.Headers.Accept.ParseAdd("application/vnd.github+json");
        return _http.SendAsync(req, HttpCompletionOption.ResponseContentRead, ct);
    }

    /// <summary>В User-Agent только знаки версии: точка, дефис, буквы и цифры.</summary>
    internal static string SafeVersion(string v)
    {
        var s = new string(v.Where(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '-').ToArray());
        return s.Length > 0 ? s : "0";
    }

    internal static UpdateResult Decide(ReleaseInfo release, string current)
    {
        var remote = SemVer.Parse(release.Tag);
        if (remote is null) return new(UpdateState.Failed);
        var mine = SemVer.Parse(current);
        if (mine is not null && remote.CompareTo(mine) <= 0) return new(UpdateState.UpToDate, remote.ToString());
        return new(UpdateState.NewVersion, remote.ToString(), PageUrl(release));
    }

    /// <summary>
    /// Страница выпуска для браузера. Адрес из ответа берётся, только если он на странице выпусков
    /// Zamok на github.com; иначе адрес собирается из тега.
    /// </summary>
    internal static string PageUrl(ReleaseInfo release)
    {
        if (release.HtmlUrl is { } u && u.StartsWith(ReleasesPage + "/", StringComparison.Ordinal) &&
            Uri.TryCreate(u, UriKind.Absolute, out var parsed) && parsed.Host == "github.com" && parsed.Scheme == Uri.UriSchemeHttps)
            return u;
        return ReleasesPage + "/tag/" + Uri.EscapeDataString(release.Tag);
    }

    /// <summary>Ответ <c>/releases/latest</c>: null, если это не выпуск, черновик или нет тега.</summary>
    internal static ReleaseInfo? ParseRelease(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            return FromElement(doc.RootElement);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>Ответ <c>/releases</c>: выпуски без черновиков по порядку; null, если ответ не список.</summary>
    internal static List<ReleaseInfo>? ParseList(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            if (doc.RootElement.ValueKind != JsonValueKind.Array) return null;
            var list = new List<ReleaseInfo>();
            foreach (var e in doc.RootElement.EnumerateArray())
                if (FromElement(e) is { } r) list.Add(r);
            return list;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private static ReleaseInfo? FromElement(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        if (Flag(e, "draft")) return null;
        if (!e.TryGetProperty("tag_name", out var tag) || tag.ValueKind != JsonValueKind.String) return null;
        var t = tag.GetString();
        if (string.IsNullOrWhiteSpace(t)) return null;
        string? url = e.TryGetProperty("html_url", out var h) && h.ValueKind == JsonValueKind.String ? h.GetString() : null;
        return new ReleaseInfo(t, url, Flag(e, "prerelease"));
    }

    private static bool Flag(JsonElement e, string name) =>
        e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
}

/// <summary>
/// Номер версии по правилам SemVer 2.0: <c>1.2.3</c>, <c>1.2.3-alpha.2</c>, ведущая <c>v</c> из тега
/// и часть после <c>+</c> не учитываются. Предварительная версия младше выпуска с тем же номером.
/// </summary>
public sealed class SemVer : IComparable<SemVer>
{
    public int Major { get; }
    public int Minor { get; }
    public int Patch { get; }
    public IReadOnlyList<string> Pre { get; }

    private SemVer(int major, int minor, int patch, string[] pre)
    {
        Major = major;
        Minor = minor;
        Patch = patch;
        Pre = pre;
    }

    public static SemVer? Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text)) return null;
        var s = text.Trim();
        if (s[0] is 'v' or 'V') s = s[1..];
        var plus = s.IndexOf('+');
        if (plus >= 0) s = s[..plus];
        string[] pre = [];
        var dash = s.IndexOf('-');
        if (dash >= 0)
        {
            pre = s[(dash + 1)..].Split('.');
            s = s[..dash];
            if (pre.Any(p => p.Length == 0 || !p.All(c => char.IsAsciiLetterOrDigit(c) || c == '-'))) return null;
        }
        var parts = s.Split('.');
        if (parts.Length is < 1 or > 3) return null;
        var n = new int[3];
        for (var i = 0; i < parts.Length; i++)
        {
            if (parts[i].Length == 0 || !parts[i].All(char.IsAsciiDigit) || !int.TryParse(parts[i], out n[i])) return null;
        }
        return new SemVer(n[0], n[1], n[2], pre);
    }

    public int CompareTo(SemVer? other)
    {
        if (other is null) return 1;
        var c = Major.CompareTo(other.Major);
        if (c == 0) c = Minor.CompareTo(other.Minor);
        if (c == 0) c = Patch.CompareTo(other.Patch);
        if (c != 0) return c;
        // Выпуск старше любой своей предварительной версии
        if (Pre.Count == 0 || other.Pre.Count == 0) return other.Pre.Count.CompareTo(Pre.Count);
        for (var i = 0; i < Math.Min(Pre.Count, other.Pre.Count); i++)
        {
            var a = Pre[i];
            var b = other.Pre[i];
            var an = a.All(char.IsAsciiDigit);
            var bn = b.All(char.IsAsciiDigit);
            if (an && bn)
            {
                // Длинное число больше короткого (без переполнения int), затем по знакам
                c = a.TrimStart('0').Length.CompareTo(b.TrimStart('0').Length);
                if (c == 0) c = string.CompareOrdinal(a.TrimStart('0'), b.TrimStart('0'));
            }
            else if (an != bn)
            {
                c = an ? -1 : 1;
            }
            else
            {
                c = string.CompareOrdinal(a, b);
            }
            if (c != 0) return c;
        }
        return Pre.Count.CompareTo(other.Pre.Count);
    }

    public override string ToString() =>
        $"{Major}.{Minor}.{Patch}" + (Pre.Count > 0 ? "-" + string.Join('.', Pre) : "");
}
