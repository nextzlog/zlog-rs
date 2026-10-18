# Data of zlog-world

ARDX.DAT, ARRL10.DAT and IOTA.DAT are zLog's resource files (zlog/resource, MIT License, JARL Contest Committee and contributors), taken from the zLog source at commit 6e309daf.
Their first line names the file and its version date (2022/11/30, 2022/12/01, 2022/11/30).
IOTA.DAT was converted from Shift_JIS to UTF-8 with `iconv -f sjis -t utf8`; the other two are copied unchanged.

CTY.DAT is not included.
It is AD1C's country file from country-files.com, and its terms of use found on 2026-10-19 do not state that it may be redistributed.
zLog does not ship it either; the user downloads it.
The program reads the user's copy from `$ZLOG_CTY_DAT`, or `CTY.DAT` next to the executable, or in the working directory (`cty::Cty::default_path`).
The test fixture in `src/cty.rs` is a small self-made file in the same format, not AD1C's data.
