# Contributors in the About window

**About ▸ Contributors** credits the people who contributed to VectorCraft. The table contains
verified GitHub usernames and names that contributors chose for themselves.

## Decision

- Two files, both **compiled into the binary** by the UI crate's `build.rs` (nothing is read from
  disk or the network at run time; the web build has them too):
  - [`contributors/contributors.json`](../contributors/contributors.json): commit stats per GitHub
    username. Refresh with the statistics generator, then run `contributors/prepare.py` below.
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

or, with craftrules checked out next to `craft-apps/`:

```sh
python3 ../../craftrules/scripts/contributors.py --add-me . --real-name "Your Name" --sync-github-name
```

Names show after the next refresh: the script checks that each entry was committed by its own
user (or by a maintainer with a `consent_source`). Delete your entry to remove your names.

## Refreshing (maintainers)

```sh
python3 ../../craftrules/scripts/contributors.py .           # regenerate + verify; needs git, Python 3.11+, gh
python3 contributors/prepare.py                             # prepare the local human contributor table
python3 ../../craftrules/scripts/contributors.py --check .   # verify people.toml only
python3 contributors/prepare.py --check                     # validate the prepared statistics
```

Commit the updated `contributors/contributors.json`. Git author names and emails are never recorded.
