Files: 20000 small files (1-8 KB), big file: 1024 MB. Seconds. 2026-11-07 22:38

| Variant | copy tree in (robocopy) | list tree (dir /s /b) | git add + commit | git status (warm) | copy 1 GB file in | read 1 GB file | delete tree | |---|---|---|---|---|---|---|---|
| NTFS | 7,06 | 1,49 | 147,33 | 0,12 | 2,64 | 1,06 | 14,07 |
| WinFsp, cache 0 ms | 165,26 | 2,53 | 1011,99 | 0,27 | 13,39 | 4,66 | 426,8 |
| WinFsp, cache 1000 ms | 143,91 | 2,37 | 448,92 | 0,19 | 2,07 | 1,59 | 15,63 |
| WinFsp, cache infinite | 141,89 | 1,29 | 420,16 | 0,19 | 2,4 | 1,4 | 15,32 |
