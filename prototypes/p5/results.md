Files: 20000 small files (1-8 KB), big file: 1024 MB. Seconds. 2026-11-07 23:27

| Variant | copy tree in (robocopy) | list tree (dir /s /b) | git add + commit | git status (warm) | copy 1 GB file in | read 1 GB file | delete tree |
|---|---|---|---|---|---|---|---|
| NTFS | 8,2 | 0,51 | 164,95 | 0,09 | 40,56 | 0,98 | 2,19 |
| WinFsp, cache 0 ms | 100,76 | 1,36 | 417,21 | 0,18 | 3,08 | 1,4 | 13,54 |
| WinFsp, cache 1000 ms | 95,29 | 1,15 | 326,94 | 0,16 | 1,81 | 1,71 | 15,63 |
| WinFsp, cache infinite | 141,59 | 1,1 | 302,79 | 0,16 | 1,99 | 1,43 | 13,53 |
