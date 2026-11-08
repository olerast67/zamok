# Zamok

Folder encryption for Windows 10 and 11, right from File Explorer. Right-click a folder and choose "Encrypt with password". Double-click, enter the password or use your fingerprint, and the folder opens in its usual place. Lock the screen and it is locked again.

The project is in development: there is an alpha to try, the first public version is not out yet. Keep a copy of important data outside Zamok for now.

## What it protects from

It protects against:
- a stolen or lost laptop that is turned off;
- copies of the folder, USB drives, cloud storage;
- another user of the same computer;
- someone who sits down at your computer while the folder is locked.

It does not protect against:
- malware on your computer while the folder is open;
- someone who knows the password or your Windows Hello PIN;
- deletion: an encrypted folder cannot be read, but it can be deleted.

## What you should know

Lock all folders at once: Ctrl+Alt+Shift+L.

**Do not drag an open folder to another place with the mouse.** Explorer copies the files there unencrypted and deletes them from the vault. Lock the folder first: a locked folder has nothing to copy and stays where it is. Zamok itself cancels "Cut" on an encrypted folder and moving a parent folder to another drive. Windows offers no way to intercept dragging the folder itself, and other file managers do not see these checks.

Zamok never writes decrypted data to disk itself, but Windows and other programs leave traces:

| Where | What happens |
|---|---|
| Windows Search index | Files are marked as "do not index" |
| Thumbnail cache | Explorer may store small copies of pictures on the system drive; this cannot be turned off for a single folder |
| Recent files | Shortcuts to the folder's files are removed on lock; taskbar jump lists stay for now |
| Autosave and temporary files | Word, Photoshop and others write them to their own folders outside Zamok |
| Page file and hibernation file | Contents of memory may end up there |
| Recycle Bin | There is none: a file deleted from an open folder is gone at once |

Encrypting the system drive (BitLocker or Device Encryption) covers almost all of these traces if the laptop is stolen.

## How it is different

- The folder stays at its path. Shortcuts, projects in your programs and recent files keep working.
- No volumes, drive letters or vaults to move your files into.
- It is real encryption: without the password the files cannot be read by another program or on another computer. Many "folder lockers" only hide files.
- Decrypted files are not written to disk, they live in memory while the folder is open.
- Cryptomator storage format: if Zamok stops working, Cryptomator opens the folder on Windows, macOS, Linux, iOS or Android.
- Open source, no telemetry, no accounts, no subscription.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Report vulnerabilities privately: [SECURITY.md](SECURITY.md). Design documents are in Russian, in [docs](docs/).

## License

GPLv3, see [LICENSE](LICENSE).

Zamok uses WinFsp: WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos, https://github.com/winfsp/winfsp
