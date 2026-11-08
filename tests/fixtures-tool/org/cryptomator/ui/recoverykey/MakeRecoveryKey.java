// Ключ восстановления для эталонного хранилища, созданный кодом самого Cryptomator
// (RecoveryKeyFactory из cryptomator-1.19.6.jar). Класс лежит в том же пакете,
// потому что WordEncoder в Cryptomator не публичный.
package org.cryptomator.ui.recoverykey;

import org.cryptomator.cryptolib.common.MasterkeyFileAccess;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.SecureRandom;

public class MakeRecoveryKey {
    public static void main(String[] args) throws Exception {
        var factory = new RecoveryKeyFactory(new WordEncoder(), new MasterkeyFileAccess(new byte[0], new SecureRandom()));
        String key = factory.createRecoveryKey(Paths.get(args[0]), args[1]);
        Files.writeString(Paths.get(args[2]), key);
        System.out.println("ok, words " + key.split(" ").length + ", valid=" + factory.validateRecoveryKey(key));
    }
}
