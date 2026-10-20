# Contributors in the About window

**About ▸ Contributors** credits the people who contributed to VectorCraft. The table contains
verified GitHub usernames and names that contributors chose for themselves.

## Decision

- Two files, both **compiled into the binary** by the UI crate's `build.rs` (nothing is read from
  disk or the network at run time; the web build has them too):
  - [`contributors/contributors.json`](../contributors/contributors.json): commit stats per GitHub
    username. A maintainer may import a reviewed snapshot, then run `contributors/prepare.py` below.
    Keep individual contributor records unchanged when preparing the generated file.
  - [`contributors/people.toml`](../contributors/people.toml): the names people chose for
    themselves. **Each contributor adds only their own entry**, in their own PR.
- Each contributor gets one line: GitHub username (always), display name (opt-in, or their synced
  GitHub profile name), real name (only if they told us), merged PRs, commits, lines added, lines
  deleted, binary assets added, binary assets removed, first and last commit date.
- The list is shown as a **grab bag** (names flowing as text) or a **table**, in the same order. It
  sorts by name, PRs, commits, lines added, lines deleted, line delta, binary assets added/removed,
  first or last commit date. The name toggle cycles **Username → Display name → Real name**; a
  missing name falls back to `@username`. Alphabetical sorting is case-insensitive and ignores the `@`.

## In VectorCraft

- The About window is `crates/ui-egui/src/dialogs/about.rs` (Help ▸ About VectorCraft). The labels are
  translated like the rest of the UI (`tl!`, `crates/ui-egui/src/i18n`); names and numbers are data.

## Credit yourself

Add your entry to `contributors/people.toml` in a PR you commit yourself (every field optional):

```toml
[people.your-github-username]
real_name = "Your Name"
display_name = "Nickname"
sync_github_name = true   # use the name on your public GitHub profile as display name
```

Maintainers verify that each requested name was submitted by its own contributor (or with a
recorded `consent_source`) before marking that entry verified in the statistics snapshot.
Delete your own entry to remove your names from the next build.

## Refreshing (maintainers)

This repository does not include a Git-history statistics generator. Keep the last verified
snapshot until a maintainer can import a reviewed refresh, with verified consent and all existing
human contributor records preserved. Building the application does not need another repository.

Prepare an imported snapshot using the local helper:

```sh
python3 contributors/prepare.py
python3 contributors/prepare.py --check
```

The helper retains only the human statistics table and supported metadata, updates the repository
identity and verifies that preparation is complete. It does not calculate new commit totals, fetch
profile names, verify consent or change individual human records. Consent review remains a
maintainer responsibility. CI runs its `--check` mode.

Commit the updated `contributors/contributors.json`. Git author names and emails are never recorded.
