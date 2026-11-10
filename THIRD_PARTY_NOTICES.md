# Third-party components

Zamok is licensed under the GNU GPL v3.0 (see [LICENSE](LICENSE)). The installer and the programs contain the components below under their own licenses.

## File system driver

WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos, https://github.com/winfsp/winfsp

| Component | Used for | License |
|---|---|---|
| WinFsp 2.1 (`winfsp-2.1.25156.msi`), unmodified | Driver that presents an encrypted folder as a file system. The Zamok installer contains this MSI and installs it if WinFsp is absent. | GPL-3.0 with the WinFsp FLOSS exception |

`installer/build-msi.ps1` accepts the MSI only with the recorded SHA-256 and a valid publisher signature. The file is not stored in this repository.

## Installer

| Component | Used for | License |
|---|---|---|
| WiX Toolset 5.0.2, including the Burn engine and the standard bootstrapper application | Builds the MSI and `Zamok-Setup-<version>.exe`. The Burn engine is part of the setup program. | MS-RL |
| `installer/thm.xml` | Installer theme, derived from the WiX theme. The file keeps its .NET Foundation copyright header and stays under MS-RL. | MS-RL |

## Windows interface (NuGet, `Zamok.exe`)

| Package | Version | Used for | License |
|---|---|---|---|
| Microsoft.WindowsAppSDK and its component packages | 2.5.1 | WinUI 3 windows, copied next to `Zamok.exe` | Microsoft Software License Terms for the Windows App SDK (`license.txt` in the package), redistributable files |
| .NET runtime, self-contained in the published app | 10 | Runtime of `Zamok.exe` | MIT |
| CommunityToolkit.Mvvm | 8.4.2 | Interface helpers | MIT |
| Microsoft.Windows.SDK.BuildTools | 10.0.28000.2705 | Build only, not shipped | Microsoft Software License Terms |

## Rust crates in `zamok-core`, `zamok-recover` and `zamok-shell.dll`

Generated from `core/Cargo.lock` with `cargo tree -e normal --target x86_64-pc-windows-msvc` and the license fields in `cargo metadata`. Build-only and test-only crates are left out. 101 crates.

| Crate | Version | License | Source |
|---|---|---|---|
| aead | 0.5.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| aes | 0.8.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-ciphers |
| aes-gcm | 0.10.3 | Apache-2.0 OR MIT | https://github.com/RustCrypto/AEADs |
| aes-kw | 0.2.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/key-wraps/tree/aes-kw |
| aes-siv | 0.7.0 | Apache-2.0 OR MIT | https://github.com/RustCrypto/AEADs |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | https://github.com/dtolnay/anyhow |
| base64 | 0.22.1 | MIT OR Apache-2.0 | https://github.com/marshallpierce/rust-base64 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| cipher | 0.4.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| cmac | 0.7.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/MACs |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.4 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| ctr | 0.9.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-modes |
| data-encoding | 2.11.1 | MIT | https://github.com/ia0/data-encoding |
| dbl | 0.3.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| digest | 0.10.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| either | 1.19.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/either |
| generic-array | 0.14.7 | MIT | https://github.com/fizyk20/generic-array |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| ghash | 0.5.1 | Apache-2.0 OR MIT | https://github.com/RustCrypto/universal-hashes |
| hmac | 0.12.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/MACs |
| inout | 0.1.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| itoa | 1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | https://github.com/rust-lang-nursery/lazy-static.rs |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| opaque-debug | 0.3.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| paste | 1.0.15 | MIT OR Apache-2.0 | https://github.com/dtolnay/paste |
| pbkdf2 | 0.12.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/password-hashes/tree/master/pbkdf2 |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite |
| polyval | 0.6.2 | Apache-2.0 OR MIT | https://github.com/RustCrypto/universal-hashes |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| quote | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| rayon | 1.12.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| rpassword | 7.5.4 | Apache-2.0 | https://github.com/conradkleinespel/rpassword |
| rtoolbox | 0.0.6 | Apache-2.0 | https://github.com/conradkleinespel/rtoolbox |
| salsa20 | 0.10.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/stream-ciphers |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| scrypt | 0.11.0 | MIT OR Apache-2.0 | https://github.com/RustCrypto/password-hashes/tree/master/scrypt |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| sha1 | 0.10.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| sharded-slab | 0.1.7 | MIT | https://github.com/hawkw/sharded-slab |
| smallvec | 1.16.5 | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs |
| subtle | 2.6.1 | BSD-3-Clause | https://github.com/dalek-cryptography/subtle |
| syn | 2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| syn | 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | https://github.com/Amanieu/thread_local-rs |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec |
| tracing | 0.1.44 | MIT | https://github.com/tokio-rs/tracing |
| tracing-attributes | 0.1.31 | MIT | https://github.com/tokio-rs/tracing |
| tracing-core | 0.1.36 | MIT | https://github.com/tokio-rs/tracing |
| tracing-subscriber | 0.3.23 | MIT | https://github.com/tokio-rs/tracing |
| typenum | 1.20.1 | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| unicode-normalization | 0.1.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/unicode-rs/unicode-normalization |
| universal-hash | 0.5.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| widestring | 1.2.1 | MIT OR Apache-2.0 | https://github.com/VoidStarKat/widestring-rs |
| windows | 0.61.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-collections | 0.2.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-future | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | 0.1.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-numerics | 0.2.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.3.4 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | 0.4.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-threading | 0.1.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| winfsp | 0.13.2+winfsp-2.1 | GPL-3.0 | https://github.com/SnowflakePowered/winfsp-rs |
| winfsp-sys | 0.12.1+winfsp-2.1 | GPL-3.0 | https://github.com/SnowflakePowered/winfsp-rs |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | https://github.com/RustCrypto/utils |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT | https://github.com/RustCrypto/utils |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij |
