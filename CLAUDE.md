# TermConnect

## Commit messages

Follow Conventional Commits for the subject line:

```
<type>(<scope>): <short description>

<body>
```

- **type**: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`, `style` — pick the one that matches the change's nature.
- **scope**: the area touched (e.g. `app`, `tui`, `connection`, `terminal`, `config`) — lowercase, matches the module/directory most affected.
- **short description**: imperative mood, lowercase, no trailing period, fits on one line with the prefix.
- **body**: explain *what changed* and *why it was needed* — the motivation, the problem being solved, the constraint being satisfied. Do not describe the mechanical process that produced the commit (e.g. "Fix final review findings", "Address review comments", "Apply requested changes") — a reader six months from now has no idea what "the review" was or what it found. State the actual change and its reason as if the review/discussion never happened.

Example — bad (describes the process, not the change):
```
Fix final review findings: resolve dangling doc link, drop unneeded pub(super) on render_title
```

Example — good (describes the change and why):
```
docs(app): fix broken link and drop unneeded visibility in render_title

The repo-restructure spec doc pointed at a path that moved during the
split. render_title was marked pub(super) from the same split but has
no caller outside its own module, so narrow it to private.
```
