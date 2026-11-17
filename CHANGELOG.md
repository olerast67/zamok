# Changelog

Changes to Zamok by version. Release notes on GitHub are taken from the section of that version.

## 0.2.0-alpha (2026-11-17)

Second alpha. Not audited by a third party. Keep a backup of important data outside Zamok.

USB sticks and memory cards
- Folders on exFAT and FAT32 drives can be encrypted. The folder on the stick keeps one file, `Открыть Name.zamok`; double-clicking it and entering the password opens the folder as a separate drive letter (from Z: downwards, labelled with the folder name) in a new Explorer window. Locking wipes the keys and removes the letter. All auto-lock rules apply.
- Pulling the stick out or pressing "Eject" while the folder is open locks it at once.
- A folder whose stick is not plugged in stays in the list as "not connected" and is found again under another drive letter by the volume serial number.
- On another computer with Zamok, double-clicking the opener file and entering the password adds the folder to that computer's list.
- On FAT32, folders with a file that would exceed 4 GB once encrypted are refused before copying.
- Tested on NTFS with a test switch and on a real exFAT stick, including "Eject" and pulling the stick out by hand with the folder open. Not tested yet: FAT32, a file over 4 GB on exFAT, a different drive letter on a real stick, a second computer.

Fixes
- After Zamok stopped without a clean shutdown, the folder could stay "unavailable" with "the path is occupied": the leftover empty folder had the read-only attribute, so Windows refused to remove it. Zamok now clears the attribute and removes such an empty folder.

## 0.1.0-alpha (2026-11-10)

First alpha. Not audited by a third party. Keep a backup of important data outside Zamok.

Encryption
- Encrypt a folder in place from the Explorer right-click menu. The original is deleted only after the copy is verified. A crash or a failure while copying keeps the files already verified, and encrypting again with the same password continues where it stopped; a crash after verification finishes the work.
- Cryptomator vault format 8 through RustCrypto. Cryptomator opens Zamok folders and Zamok opens Cryptomator vaults.
- Recovery key of 44 words, optional, on by default.
- Change password, show recovery key, decrypt permanently.
- `zamok-recover`, a command-line export of a decrypted copy, built from source.

Daily use
- The folder stays at its path through WinFsp. Locked folders show one icon that opens the password window.
- Windows Hello as a second way in, turned on per folder.
- Auto-lock on screen lock, sleep, hibernation and sign-out, and after 30 minutes without work in the folder (set per folder). Ctrl+Alt+Shift+L locks all folders.
- If files are open when the timeout comes, Zamok asks whether to lock now or postpone for 10 minutes. The lock prompt shows which programs hold files open.
- Tray icon, notifications, settings window. Interface in Russian and English.
- "Check for updates" button in the settings: one request to GitHub, only when you press it. Nothing is downloaded; a newer version opens its release page.
- Files are marked "do not index"; shortcuts in Recent files are removed on lock.
- "Traces check" in the settings shows, without changing anything, whether the system drive is encrypted, hibernation is on, the page file is encrypted, your folders are in the search index and Explorer keeps thumbnails.
- Keys are kept in memory that Windows does not page out and are wiped on lock.
- Large files read at about 70% of NTFS speed thanks to read-ahead (bench/RESULTS.md).

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
