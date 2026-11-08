fn main() {
    // WinFsp загружается при первом вызове: без установленного WinFsp программа не падает при старте
    winfsp::build::winfsp_link_delayload();
}
