# Changelog

Changes to Zamok by version. Release notes on GitHub are taken from the section of that version.

## 0.1.0-alpha (not released yet)

First alpha. Not audited by a third party. Keep a backup of important data outside Zamok.

Encryption
- Encrypt a folder in place from the Explorer right-click menu. The original is deleted only after the copy is verified; a crash before that rolls back, a crash after that finishes the work.
- Cryptomator vault format 8 through RustCrypto. Cryptomator opens Zamok folders and Zamok opens Cryptomator vaults.
- Recovery key of 44 words, optional, on by default.
- Change password, show recovery key, decrypt permanently.
- `zamok-recover`, a command-line export of a decrypted copy, built from source.

Daily use
- The folder stays at its path through WinFsp. Locked folders show one icon that opens the password window.
- Windows Hello as a second way in, turned on per folder.
- Auto-lock on screen lock, sleep, hibernation and sign-out, and after 30 minutes without work in the folder (set per folder). Ctrl+Alt+Shift+L locks all folders.
- Tray icon, notifications, settings window. Interface in Russian and English.
- "Check for updates" button in the settings: one request to GitHub, only when you press it. Nothing is downloaded; a newer version opens its release page.
- Files are marked "do not index"; shortcuts in Recent files are removed on lock.

Safety checks
- Zamok cancels "Cut" on an encrypted folder and moving a parent folder to another drive.
- Before encrypting, Zamok checks for NTFS, free space, open files, system folders, cloud-only files and folders inside OneDrive, Dropbox and Yandex Disk.

Install
- `Zamok-Setup-<version>.exe` contains WinFsp and installs it only if absent. Zamok installs per user. Not signed.
- Uninstalling leaves folders encrypted and writes a note next to each one about how to open it.
- `SHA256SUMS.txt` next to the installer lists the SHA-256 of every release file. The Rust programs build bit for bit the same from any folder (RELEASING.md).

Known limits
- Dragging an open folder with the mouse in Explorer copies its files unencrypted and deletes them from the vault. See the warning in the README.
- NTFS only. No signature, no automatic update install. Tested on Windows 10; Windows 11 and cloud clients are not tested yet.
