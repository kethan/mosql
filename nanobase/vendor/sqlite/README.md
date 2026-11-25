# SQLite

The official SQLite amalgamation, **3.46.0** (`SQLITE_VERSION` in `sqlite3.h`),
in public domain.

Source: the copy shipped by `rusqlite` 0.32.1
(`libsqlite3-sys/sqlite3/sqlite3.{c,h}`, taken from
<https://github.com/rusqlite/rusqlite/tree/v0.32.1>), which is itself the
unmodified amalgamation from <https://www.sqlite.org/download.html>.

`../../build.rs` compiles `sqlite3.c` with a fixed set of `SQLITE_OMIT_*`
options; see the comments there. To upgrade, replace both files with the newer
amalgamation and re-run `./verify.sh`.
