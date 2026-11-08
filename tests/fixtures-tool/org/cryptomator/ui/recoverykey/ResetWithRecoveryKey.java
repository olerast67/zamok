// Проверка ключа восстановления, созданного Zamok, кодом самого Cryptomator:
// ключ проходит проверку, и по нему Cryptomator задаёт хранилищу новый пароль.
package org.cryptomator.ui.recoverykey;

import org.cryptomator.cryptolib.common.MasterkeyFileAccess;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.SecureRandom;

public class ResetWithRecoveryKey {
    public static void main(String[] args) throws Exception {
        var factory = new RecoveryKeyFactory(new WordEncoder(), new MasterkeyFileAccess(new byte[0], new SecureRandom()));
        String key = Files.readString(Paths.get(args[1])).trim();
        System.out.println("valid=" + factory.validateRecoveryKey(key));
        factory.newMasterkeyFileWithPassphrase(Paths.get(args[0]), key, args[2]);
        System.out.println("new password set");
    }
}
