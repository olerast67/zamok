using System.Net;
using Zamok.Client;

namespace Zamok.Tests;

/// <summary>Сравнение версий по SemVer: тег GitHub против версии запущенной программы.</summary>
public class SemVerTests
{
    [Theory]
    [InlineData("0.1.0", "0.1.0-alpha")]
    [InlineData("0.1.0-alpha.2", "0.1.0-alpha")]
    [InlineData("0.1.0-alpha.10", "0.1.0-alpha.2")]
    [InlineData("0.1.0-beta", "0.1.0-alpha.9")]
    [InlineData("0.1.0-alpha.beta", "0.1.0-alpha.1")]
    [InlineData("0.1.0-rc.1", "0.1.0-beta.11")]
    [InlineData("0.2.0-alpha", "0.1.0")]
    [InlineData("1.0.0", "0.99.99")]
    [InlineData("0.1.1", "0.1.0")]
    [InlineData("0.10.0", "0.9.0")]
    [InlineData("v0.2.0", "0.1.0-alpha")]
    [InlineData("0.1.0-alpha.99999999999", "0.1.0-alpha.9999999999")]
    public void NewerIsGreater(string newer, string older)
    {
        Assert.True(SemVer.Parse(newer)!.CompareTo(SemVer.Parse(older)) > 0, $"{newer} > {older}");
        Assert.True(SemVer.Parse(older)!.CompareTo(SemVer.Parse(newer)) < 0, $"{older} < {newer}");
    }

    [Theory]
    [InlineData("0.1.0-alpha", "v0.1.0-alpha")]
    [InlineData("0.1.0-alpha+abc123", "0.1.0-alpha")]
    [InlineData("1.2", "1.2.0")]
    [InlineData("1", "1.0.0")]
    public void SameVersion(string a, string b) =>
        Assert.Equal(0, SemVer.Parse(a)!.CompareTo(SemVer.Parse(b)));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("v")]
    [InlineData("latest")]
    [InlineData("1.2.3.4")]
    [InlineData("1..2")]
    [InlineData("1.2.x")]
    [InlineData("1.2.3-")]
    [InlineData("1.2.3-alpha..1")]
    [InlineData("1.2.3-альфа")]
    [InlineData("-1.2.3")]
    [InlineData("99999999999.0.0")]
    public void GarbageIsNotAVersion(string? text) => Assert.Null(SemVer.Parse(text));

    [Fact]
    public void ToStringDropsTheVAndBuild() =>
        Assert.Equal("0.2.0-alpha.1", SemVer.Parse("v0.2.0-alpha.1+sha.5114f85")!.ToString());
}

/// <summary>Разбор ответов API GitHub и итог проверки, без сети: обработчик HTTP подменён.</summary>
public class UpdateCheckTests
{
    private const string Latest = UpdateChecker.GitHubBase + "/releases/latest";
    private const string List = UpdateChecker.GitHubBase + "/releases?per_page=5";

    private static string Release(string tag, bool draft = false, bool pre = false, string? url = null) =>
        $$"""{"tag_name":"{{tag}}","draft":{{(draft ? "true" : "false")}},"prerelease":{{(pre ? "true" : "false")}},"html_url":"{{url ?? UpdateChecker.ReleasesPage + "/tag/" + tag}}","assets":[]}""";

    [Fact]
    public void ParsesARelease()
    {
        var r = UpdateChecker.ParseRelease(Release("v0.2.0", pre: true));
        Assert.NotNull(r);
        Assert.Equal("v0.2.0", r.Tag);
        Assert.True(r.Prerelease);
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v0.2.0", r.HtmlUrl);
    }

    [Theory]
    [InlineData("""{"tag_name":"v1.0.0","draft":true}""")]
    [InlineData("""{"draft":false,"html_url":"https://github.com/olerast67/zamok/releases/tag/v1"}""")]
    [InlineData("""{"tag_name":null}""")]
    [InlineData("""{"tag_name":42}""")]
    [InlineData("""{"tag_name":"  "}""")]
    [InlineData("""{"message":"Not Found","documentation_url":"https://docs.github.com"}""")]
    [InlineData("""[]""")]
    [InlineData("""null""")]
    [InlineData("""<html>rate limited</html>""")]
    [InlineData("""{"tag_name":"v1.0.0" """)]
    [InlineData("")]
    public void NotAReleaseGivesNull(string json) => Assert.Null(UpdateChecker.ParseRelease(json));

    [Fact]
    public void MissingOptionalFieldsAreFine()
    {
        var r = UpdateChecker.ParseRelease("""{"tag_name":"v0.3.0"}""");
        Assert.NotNull(r);
        Assert.False(r.Prerelease);
        Assert.Null(r.HtmlUrl);
    }

    [Fact]
    public void ListSkipsDraftsAndBrokenEntries()
    {
        var json = $"[{Release("v0.4.0", draft: true)},{{\"name\":\"no tag\"}},17,{Release("v0.3.0-alpha", pre: true)},{Release("v0.2.0")}]";
        var list = UpdateChecker.ParseList(json);
        Assert.NotNull(list);
        Assert.Equal(["v0.3.0-alpha", "v0.2.0"], list.Select(r => r.Tag));
    }

    [Theory]
    [InlineData("""{"message":"Not Found"}""")]
    [InlineData("""garbage""")]
    [InlineData("")]
    public void ListThatIsNotAListGivesNull(string json) => Assert.Null(UpdateChecker.ParseList(json));

    [Fact]
    public void PageUrlOnlyFromZamokReleases()
    {
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v1.0.0",
            UpdateChecker.PageUrl(new("v1.0.0", UpdateChecker.ReleasesPage + "/tag/v1.0.0", false)));
        // Чужой адрес или его нет: адрес из тега
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v1.0.0", UpdateChecker.PageUrl(new("v1.0.0", "https://evil.example/zamok", false)));
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v1.0.0", UpdateChecker.PageUrl(new("v1.0.0", "https://github.com/olerast67/zamok/releases@evil.example/x", false)));
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v1.0.0", UpdateChecker.PageUrl(new("v1.0.0", null, false)));
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v1.0%20x", UpdateChecker.PageUrl(new("v1.0 x", null, false)));
    }

    [Theory]
    [InlineData("http://127.0.0.1:5123/repos/x", true)]
    [InlineData("http://localhost:5123", true)]
    [InlineData("http://[::1]:5123", true)]
    [InlineData("https://api.github.com/repos/olerast67/zamok", false)]
    [InlineData("http://192.168.1.10:80", false)]
    [InlineData("file:///C:/x", false)]
    [InlineData("not a url", false)]
    [InlineData("", false)]
    [InlineData(null, false)]
    public void TestAddressOnlyOnThisComputer(string? url, bool accepted) =>
        Assert.Equal(accepted ? url : null, UpdateChecker.LoopbackOnly(url));

    [Fact]
    public void RunningVersionDropsTheCommit()
    {
        var v = UpdateChecker.RunningVersion(typeof(UpdateChecker).Assembly);
        Assert.DoesNotContain("+", v);
        Assert.NotNull(SemVer.Parse(v));
        Assert.Equal("0.0.0", UpdateChecker.RunningVersion(null));
    }

    [Fact]
    public void UserAgentVersionHasOnlySafeCharacters()
    {
        Assert.Equal("0.1.0-alpha", UpdateChecker.SafeVersion("0.1.0-alpha"));
        Assert.Equal("0.1.0-alphaX", UpdateChecker.SafeVersion("0.1.0-alpha\r\nX"));
        Assert.Equal("0", UpdateChecker.SafeVersion(" "));
    }

    // Проверка целиком с подменённым обработчиком

    private sealed class FakeHandler(Func<HttpRequestMessage, HttpResponseMessage> answer) : HttpMessageHandler
    {
        public List<HttpRequestMessage> Requests { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            Requests.Add(request);
            return Task.FromResult(answer(request));
        }
    }

    private static HttpResponseMessage Answer(HttpStatusCode code, string body = "") =>
        new(code) { Content = new StringContent(body) };

    private static async Task<(UpdateResult, FakeHandler)> Run(string current, Func<HttpRequestMessage, HttpResponseMessage> answer)
    {
        var h = new FakeHandler(answer);
        var r = await new UpdateChecker(new HttpClient(h), current).CheckAsync();
        return (r, h);
    }

    [Fact]
    public async Task NewerLatestReleaseIsOffered()
    {
        var (r, h) = await Run("0.1.0-alpha", _ => Answer(HttpStatusCode.OK, Release("v0.2.0")));
        Assert.Equal(UpdateState.NewVersion, r.State);
        Assert.Equal("0.2.0", r.Version);
        Assert.Equal(UpdateChecker.ReleasesPage + "/tag/v0.2.0", r.Url);
        // Один запрос, только User-Agent с версией и Accept, без cookie и авторизации
        var req = Assert.Single(h.Requests);
        Assert.Equal(Latest, req.RequestUri!.ToString());
        Assert.Equal(HttpMethod.Get, req.Method);
        Assert.Equal("Zamok/0.1.0-alpha", req.Headers.UserAgent.ToString());
        Assert.Equal(["Accept", "User-Agent"], req.Headers.Select(x => x.Key).Order());
    }

    [Theory]
    [InlineData("v0.1.0-alpha")]
    [InlineData("v0.0.9")]
    public async Task SameOrOlderIsUpToDate(string tag)
    {
        var (r, h) = await Run("0.1.0-alpha", _ => Answer(HttpStatusCode.OK, Release(tag)));
        Assert.Equal(UpdateState.UpToDate, r.State);
        Assert.Single(h.Requests);
    }

    [Fact]
    public async Task OnlyPrereleasesUsesTheList()
    {
        var (r, h) = await Run("0.1.0-alpha", req => req.RequestUri!.ToString() == Latest
            ? Answer(HttpStatusCode.NotFound, """{"message":"Not Found"}""")
            : Answer(HttpStatusCode.OK, $"[{Release("v0.2.0-alpha", draft: true)},{Release("v0.1.0-alpha.2", pre: true)}]"));
        Assert.Equal(UpdateState.NewVersion, r.State);
        Assert.Equal("0.1.0-alpha.2", r.Version);
        Assert.Equal([Latest, List], h.Requests.Select(q => q.RequestUri!.ToString()));
    }

    [Fact]
    public async Task NoReleasesAtAll()
    {
        var (r, _) = await Run("0.1.0-alpha", req => req.RequestUri!.ToString() == Latest
            ? Answer(HttpStatusCode.NotFound) : Answer(HttpStatusCode.OK, "[]"));
        Assert.Equal(UpdateState.NoReleases, r.State);
    }

    [Fact]
    public async Task OnlyDraftsIsNoReleases()
    {
        var (r, _) = await Run("0.1.0-alpha", req => req.RequestUri!.ToString() == Latest
            ? Answer(HttpStatusCode.NotFound) : Answer(HttpStatusCode.OK, $"[{Release("v9.0.0", draft: true)}]"));
        Assert.Equal(UpdateState.NoReleases, r.State);
    }

    [Theory]
    [InlineData(HttpStatusCode.Forbidden, "{\"message\":\"API rate limit exceeded\"}")]
    [InlineData(HttpStatusCode.InternalServerError, "")]
    [InlineData(HttpStatusCode.OK, "<html>captive portal</html>")]
    [InlineData(HttpStatusCode.OK, "{\"tag_name\":\"latest\"}")]
    [InlineData(HttpStatusCode.OK, "{\"tag_name\":\"v1.0.0\",\"draft\":true}")]
    public async Task ErrorsAndGarbageFail(HttpStatusCode code, string body)
    {
        var (r, _) = await Run("0.1.0-alpha", _ => Answer(code, body));
        Assert.Equal(UpdateState.Failed, r.State);
    }

    [Fact]
    public async Task RepositoryNotFoundFails()
    {
        // Оба адреса 404 (репозиторий закрыт или переименован): «нет выпусков» было бы неправдой
        var (r, h) = await Run("0.1.0-alpha", _ => Answer(HttpStatusCode.NotFound, """{"message":"Not Found"}"""));
        Assert.Equal(UpdateState.Failed, r.State);
        Assert.Equal(2, h.Requests.Count);
    }

    [Fact]
    public async Task NoNetworkFails()
    {
        var (r, _) = await Run("0.1.0-alpha", _ => throw new HttpRequestException("no route"));
        Assert.Equal(UpdateState.Failed, r.State);
    }

    [Fact]
    public async Task TimeoutFails()
    {
        var (r, _) = await Run("0.1.0-alpha", _ => throw new TaskCanceledException("timeout"));
        Assert.Equal(UpdateState.Failed, r.State);
    }

    [Fact]
    public async Task TestAddressReplacesGitHub()
    {
        var h = new FakeHandler(_ => Answer(HttpStatusCode.OK, Release("v0.1.0-alpha")));
        await new UpdateChecker(new HttpClient(h), "0.1.0-alpha", "http://127.0.0.1:5000/repos/x/").CheckAsync();
        Assert.Equal("http://127.0.0.1:5000/repos/x/releases/latest", h.Requests[0].RequestUri!.ToString());
    }

    [Fact]
    public void RealClientKeepsNoCookies()
    {
        using var http = UpdateChecker.CreateHttpClient();
        Assert.Equal(TimeSpan.FromSeconds(10), http.Timeout);
        using var handler = UpdateChecker.CreateHandler();
        Assert.False(handler.UseCookies);
        Assert.False(handler.UseDefaultCredentials);
    }
}
