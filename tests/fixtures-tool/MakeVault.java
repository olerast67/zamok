// Создаёт эталонное хранилище библиотеками самого Cryptomator (cryptofs, cryptolib из
// C:\Program Files\Cryptomator\app\mods). Нужно для тестов zamok-crypto.
// Запуск: см. make-fixture.ps1 рядом.
import org.cryptomator.cryptofs.CryptoFileSystemProperties;
import org.cryptomator.cryptofs.CryptoFileSystemProvider;
import org.cryptomator.cryptolib.api.Masterkey;
import org.cryptomator.cryptolib.common.MasterkeyFileAccess;

import java.net.URI;
import java.nio.file.*;
import java.security.SecureRandom;
import java.util.HexFormat;
import java.util.stream.Stream;

public class MakeVault {
    public static void main(String[] args) throws Exception {
        Path vault = Paths.get(args[0]);
        Path src = Paths.get(args[1]);
        String password = args[2];
        Files.createDirectories(vault);
        SecureRandom rng = new SecureRandom();
        Masterkey key = Masterkey.generate(rng);
        new MasterkeyFileAccess(new byte[0], rng).persist(key, vault.resolve("masterkey.cryptomator"), password);
        CryptoFileSystemProperties props = CryptoFileSystemProperties.cryptoFileSystemProperties()
                .withKeyLoader(uri -> key.copy())
                .build();
        CryptoFileSystemProvider.initialize(vault, props, URI.create("masterkeyfile:masterkey.cryptomator"));
        try (FileSystem fs = CryptoFileSystemProvider.newFileSystem(vault, props); Stream<Path> walk = Files.walk(src)) {
            for (Path p : (Iterable<Path>) walk::iterator) {
                String rel = src.relativize(p).toString().replace('\\', '/');
                if (rel.isEmpty()) continue;
                Path dst = fs.getPath("/" + rel);
                if (Files.isDirectory(p)) Files.createDirectories(dst);
                else Files.write(dst, Files.readAllBytes(p));
            }
        }
        // Только для тестов: сырой мастер-ключ (64 байта, ключ шифрования + ключ MAC)
        Files.writeString(vault.resolveSibling(vault.getFileName() + "-masterkey-hex.txt"), HexFormat.of().formatHex(key.getEncoded()));
        System.out.println("ok");
    }
}
