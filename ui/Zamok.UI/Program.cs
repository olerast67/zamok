namespace Zamok.UI;

/// <summary>
/// Точка входа. Язык окон тот же, что у core, меню Проводника и установщика: русский, если язык
/// интерфейса Windows русский, иначе английский. Без этого Windows на третьем языке показывала бы
/// окна по-русски (язык сборки по умолчанию), а трей по-английски. Язык задаётся до запуска WinUI,
/// чтобы и подписи самой библиотеки были на языке окна.
/// </summary>
public static class Program
{
    [STAThread]
    private static void Main()
    {
        Microsoft.Windows.Globalization.ApplicationLanguages.PrimaryLanguageOverride = Lang.Code;
        XamlGeneratedProgram.XamlGeneratedMain();
    }
}
