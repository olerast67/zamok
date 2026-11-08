using System.Security.Cryptography;
using Windows.Security.Credentials;
using Windows.Security.Cryptography;

namespace Zamok.UI;

/// <summary>
/// Вход по Windows Hello (DESIGN.md, раздел 3.5). Ключ Windows Hello («Zamok») подписывает строку
/// с номером папки; подпись RSA каждый раз одинаковая, но получить её можно только после
/// PIN-кода, отпечатка или лица. SHA-256 подписи и есть ключ обёртки: им core заворачивает
/// мастер-ключ папки. Сам мастер-ключ в окно не попадает.
/// </summary>
internal static class Hello
{
    private const string CredentialName = "Zamok";

    public static async Task<bool> IsAvailableAsync()
    {
        try { return await KeyCredentialManager.IsSupportedAsync(); }
        catch { return false; }
    }

    /// <summary>Ключ обёртки для папки (base64) или null, если человек отменил или Hello не подтвердил.</summary>
    public static async Task<string?> WrapKeyAsync(string folderId, bool create)
    {
        try
        {
            var r = create
                ? await KeyCredentialManager.RequestCreateAsync(CredentialName, KeyCredentialCreationOption.FailIfExists)
                : await KeyCredentialManager.OpenAsync(CredentialName);
            // Ключ уже есть (включали для другой папки): не пересоздавать, иначе те папки
            // перестанут открываться по Hello
            if (create && r.Status == KeyCredentialStatus.CredentialAlreadyExists)
                r = await KeyCredentialManager.OpenAsync(CredentialName);
            if (r.Status != KeyCredentialStatus.Success) return null;
            var data = CryptographicBuffer.ConvertStringToBinary("Zamok folder key v1 " + folderId,
                BinaryStringEncoding.Utf8);
            var signed = await r.Credential.RequestSignAsync(data);
            if (signed.Status != KeyCredentialStatus.Success) return null;
            CryptographicBuffer.CopyToByteArray(signed.Result, out var sig);
            var key = SHA256.HashData(sig);
            var b64 = Convert.ToBase64String(key);
            CryptographicOperations.ZeroMemory(key);
            CryptographicOperations.ZeroMemory(sig);
            return b64;
        }
        catch
        {
            return null;
        }
    }
}
