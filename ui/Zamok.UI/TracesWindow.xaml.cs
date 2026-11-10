using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Zamok.Client;

namespace Zamok.UI;

/// <summary>
/// «Проверка следов» (DESIGN.md, раздел 6): зашифрован ли системный диск, включена ли
/// гибернация, шифруется ли файл подкачки, входят ли папки Zamok в индекс поиска, показывает ли
/// Проводник миниатюры. В каждой строке состояние и одно предложение, что сделать. Окно
/// открывается из настроек или <c>Zamok.exe --traces</c> и ничего не меняет.
/// </summary>
public sealed partial class TracesWindow : Window
{
    private const int DialogWidth = 760;
    private readonly CoreClient _client = new();

    public TracesWindow()
    {
        InitializeComponent();
        WindowHelper.SetupDialog(this, Root, TitleBar, TitleIcon, DialogWidth, 640);
        Title = Strings.Get("Traces_Title/Text");
        AutomationProperties.SetName(Busy, Strings.Get("A11y_Busy"));
        WindowHelper.HookKeys(Root, null, Close);
        Root.Loaded += async (_, _) =>
        {
            RecheckButton.Focus(FocusState.Programmatic);
            await CheckAsync();
        };
    }

    private async Task CheckAsync()
    {
        RecheckButton.IsEnabled = false;
        Busy.IsActive = true;
        Busy.Visibility = Visibility.Visible;
        try
        {
            string[]? folders;
            try
            {
                folders = ((await _client.StatusAsync()).Folders ?? []).Select(f => f.Path).ToArray();
            }
            catch (Exception e) when (e is CoreNotRunningException or CoreNotTrustedException)
            {
                folders = null;
            }
            // Служба поиска и оболочка могут отвечать долго: не в потоке окна
            var r = await Task.Run(() => TraceProbe.Run(folders));
            Show(r);
        }
        finally
        {
            Busy.IsActive = false;
            Busy.Visibility = Visibility.Collapsed;
            RecheckButton.IsEnabled = true;
            WindowHelper.FitHeight(this, Root, DialogWidth);
        }
    }

    private void Show(TraceProbe.Result r)
    {
        var disk = r.Disk.ToString();
        Row(TraceChecks.StateOf(r.Disk), DiskGood, DiskWarning, DiskUnknown,
            DiskStatus, Strings.Format($"Traces_Disk_{disk}", r.SystemDrive), DiskAdvice, Advice($"Traces_DiskDo_{disk}"));
        var hiber = r.Hiber.ToString();
        Row(TraceChecks.StateOf(r.Hiber), HiberGood, HiberWarning, HiberUnknown,
            HiberStatus, Strings.Get($"Traces_Hiber_{hiber}"), HiberAdvice, Advice($"Traces_HiberDo_{hiber}"));
        var page = r.PageFile.ToString();
        Row(TraceChecks.StateOf(r.PageFile), PageGood, PageWarning, PageUnknown,
            PageStatus, Strings.Get($"Traces_Page_{page}"), PageAdvice, Advice($"Traces_PageDo_{page}"));
        var index = r.Index.ToString();
        var names = string.Join(", ", r.Indexed.Select(Path.GetFileName));
        Row(TraceChecks.StateOf(r.Index), IndexGood, IndexWarning, IndexUnknown,
            IndexStatus, Strings.Format($"Traces_Index_{index}", names), IndexAdvice, Advice($"Traces_IndexDo_{index}"));
        var thumbs = r.Thumbs.ToString();
        Row(TraceChecks.StateOf(r.Thumbs), ThumbsGood, ThumbsWarning, ThumbsUnknown,
            ThumbsStatus, Strings.Get($"Traces_Thumbs_{thumbs}"), ThumbsAdvice, Advice($"Traces_ThumbsDo_{thumbs}"));
    }

    /// <summary>Совет для состояния; у хороших состояний своего совета нет: «Ничего делать не нужно».</summary>
    private static string Advice(string key)
    {
        var s = Strings.Get(key);
        return s == key ? Strings.Get("Traces_Nothing") : s;
    }

    private static void Row(TraceState state, FontIcon good, FontIcon warning, FontIcon unknown,
        TextBlock status, string statusText, TextBlock advice, string adviceText)
    {
        good.Visibility = state == TraceState.Good ? Visibility.Visible : Visibility.Collapsed;
        warning.Visibility = state == TraceState.Warning ? Visibility.Visible : Visibility.Collapsed;
        unknown.Visibility = state == TraceState.Unknown ? Visibility.Visible : Visibility.Collapsed;
        status.Text = statusText;
        advice.Text = adviceText;
    }

    private async void OnRecheckClick(object sender, RoutedEventArgs e) => await CheckAsync();

    private void OnCloseClick(object sender, RoutedEventArgs e) => Close();
}
