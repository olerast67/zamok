// Читает хранилище библиотеками Cryptomator (cryptofs, cryptolib) и выгружает файлы.
// Нужно, чтобы проверить: хранилище, созданное Zamok, открывается в Cryptomator.
import org.cryptomator.cryptofs.CryptoFileSystemProperties;
import org.cryptomator.cryptofs.CryptoFileSystemProvider;
import org.cryptomator.cryptolib.api.Masterkey;
import org.cryptomator.cryptolib.common.MasterkeyFileAccess;

import java.nio.file.*;
import java.security.SecureRandom;
import java.util.stream.Stream;

public class ReadVault {
    public static void main(String[] args) throws Exception {
        Path vault = Paths.get(args[0]);
        Path out = Paths.get(args[1]);
        String password = args[2];
        Masterkey key = new MasterkeyFileAccess(new byte[0], new SecureRandom()).load(vault.resolve("masterkey.cryptomator"), password);
        CryptoFileSystemProperties props = CryptoFileSystemProperties.cryptoFileSystemProperties()
                .withKeyLoader(uri -> key.copy())
                .build();
        int files = 0;
        try (FileSystem fs = CryptoFileSystemProvider.newFileSystem(vault, props); Stream<Path> walk = Files.walk(fs.getPath("/"))) {
            for (Path p : (Iterable<Path>) walk::iterator) {
                String rel = p.toString().substring(1);
                if (rel.isEmpty()) continue;
                Path dst = out.resolve(rel);
                if (Files.isDirectory(p)) Files.createDirectories(dst);
                else { Files.createDirectories(dst.getParent()); Files.write(dst, Files.readAllBytes(p)); files++; }
            }
        }
        System.out.println("ok, files " + files);
    }
}
