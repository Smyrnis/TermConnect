# TermConnect

A terminal file manager: local files in one panel, an SSH/SFTP connection in
the other, with saved connection profiles, bookmarks, search, and background
file transfers between the two.

## Features

- Dual-panel file browser — local filesystem on one side, a remote SFTP
  session on the other.
- Saved connection profiles (host, port, username, identity file, remote
  path, optional password), stored alongside connections read from
  `~/.ssh/config`.
- Background file transfers between local and remote, queued and
  cancellable, with progress reporting.
- Recursive filename search on both the local and remote side.
- Bookmarks for quickly jumping back to a local path or a remote path on a
  given host.
- Drop to a full `ssh` terminal session for the active connection and back,
  without losing your place in the file browser.

## Installing

Requires a Rust toolchain supporting the 2024 edition (stable is fine).

```sh
cargo build --release
```

The binary is written to `target/release/termconnect`.

Optional: install [`sshpass`](https://linux.die.net/man/1/sshpass) if you
want the F4 terminal handoff to use a saved profile password automatically,
instead of prompting for it again.

**Platform:** Linux and macOS only — the app relies on Unix file
permissions and process APIs throughout.

## Keybindings

| Key | Action |
| --- | --- |
| `F1` | Help |
| `F2` | Rename (Edit Connection on the Connections screen) |
| `F4` | Open a terminal session on the active connection |
| `F5` | Copy selected file(s) |
| `F6` | Add Connection (Connections screen) |
| `F7` | Create directory |
| `F8` / `Delete` | Delete (Delete Connection on the Connections screen) |
| `F9` | Open Connections screen |
| `F10` | Quit |
| `Tab` | Switch panel |
| `↑` / `↓` | Move selection |
| `Enter` | Open |
| `Space` | Toggle selection |
| `Esc` | Back / dismiss |
| `Ctrl+R` | Refresh |
| `Ctrl+C` | Cancel active transfer |
| `Ctrl+H` | Toggle hidden files |
| `Ctrl+S` | Cycle sort order |
| `Ctrl+D` | Bookmark current location |
| `Ctrl+B` | Open bookmarks |
| `Ctrl+F` | Search |
| `Ctrl+N` | Cycle remote session |

Press `F1` in the app at any time for the full, live list — it reflects the
actual configured bindings rather than this table.

## Configuration

Settings, saved connections, and bookmarks live under
`$XDG_CONFIG_HOME/termconnect` (or `~/.config/termconnect` if unset):

- `config.toml` — panel and key-binding settings.
- `connections.toml` — saved connection profiles.
- `bookmarks.toml` — saved bookmarks.

Logs go to `$XDG_STATE_HOME/termconnect/termconnect.log` (or
`~/.local/state/termconnect/termconnect.log`); set `RUST_LOG` to control
verbosity.

## License

MIT — see [LICENSE](LICENSE).
