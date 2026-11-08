namespace Zamok.Client;

/// <summary>
/// Грубая оценка надёжности пароля для полосы в мастере шифрования: 0 (слишком короткий)
/// до 4 (надёжный). Не проверка безопасности, а подсказка человеку: длина важнее всего,
/// повторы, последовательности и известные пароли снижают оценку.
/// </summary>
public static class PasswordStrength
{
    public const int MinLength = 8;

    private static readonly string[] Common =
    [
        "password", "пароль", "qwerty", "йцукен", "123456", "111111", "iloveyou", "admin",
        "letmein", "welcome", "monkey", "dragon", "zamok", "замок", "asdfgh", "фывапр",
    ];

    private static readonly string[] Rows =
    [
        "0123456789", "abcdefghijklmnopqrstuvwxyz", "абвгдеёжзийклмнопрстуфхцчшщъыьэюя",
        "qwertyuiop", "asdfghjkl", "zxcvbnm", "йцукенгшщзхъ", "фывапролджэ", "ячсмитьбю",
    ];

    public static int Score(string password)
    {
        if (password.Length < MinLength) return 0;
        var lower = password.ToLowerInvariant();
        double pool = 0;
        if (password.Any(char.IsAsciiLetterLower)) pool += 26;
        if (password.Any(char.IsAsciiLetterUpper)) pool += 26;
        if (password.Any(char.IsAsciiDigit)) pool += 10;
        if (password.Any(c => char.IsLetter(c) && c > 127)) pool += 33;
        if (password.Any(c => !char.IsLetterOrDigit(c))) pool += 20;

        // Длина без повторов и шаблонов: «aaaa», «1234», «qwer» почти ничего не добавляют
        var effective = 0.0;
        for (var i = 0; i < password.Length; i++)
        {
            if (i > 0 && lower[i] == lower[i - 1]) { effective += 0.2; continue; }
            if (i > 0 && InRow(lower[i - 1], lower[i])) { effective += 0.3; continue; }
            effective += 1;
        }
        var bits = effective * Math.Log2(Math.Max(pool, 10));
        foreach (var c in Common)
            if (lower.Contains(c)) bits -= c.Length * 3;

        return bits switch
        {
            < 30 => 1,
            < 45 => 2,
            < 65 => 3,
            _ => 4,
        };
    }

    private static bool InRow(char a, char b)
    {
        foreach (var row in Rows)
        {
            var i = row.IndexOf(a);
            if (i >= 0 && i + 1 < row.Length && row[i + 1] == b) return true;
        }
        return false;
    }
}
