# Releasing Reckon

## Prerequisites

- Push access to the GitHub repo
- `git` on PATH

## How to cut a release

**1. Bump the version in Cargo.toml**

```toml
# Cargo.toml
version = "0.2.0"
```

Commit it:

```sh
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to v0.2.0"
```

**2. Tag the commit**

```sh
git tag v0.2.0
git push origin master --tags
```

That's it. The `release.yml` workflow fires automatically on any `v*.*.*` tag.

**3. What the workflow does**

Builds five targets in parallel:

| Target | Platform |
|--------|----------|
| `x86_64-unknown-linux-gnu` | Linux x86\_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 (built via `cross`) |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows x86\_64 |

Each bundle includes `reckon`, `reckon-mcp`, `reckon-seed`, `README.md`, and `INSTALL.md`.  
Linux/macOS → `.tar.gz`. Windows → `.zip`.

After all builds pass, the workflow creates a GitHub Release with:
- All five archives attached
- Auto-generated release notes from commit messages since the last tag

**Pre-release tags** (e.g. `v0.2.0-rc.1`, `v0.2.0-beta.1`) are published as pre-releases automatically.

## Re-releasing / fixing a broken release

Delete the tag locally and on remote, fix the issue, re-push:

```sh
git tag -d v0.2.0
git push origin :refs/tags/v0.2.0
# fix the issue, commit
git tag v0.2.0
git push origin master --tags
```

Also delete the broken GitHub Release in the web UI before re-pushing, otherwise the workflow will fail to re-create it.

## Checking workflow status

```sh
gh run list --workflow=release.yml
gh run watch          # stream latest run logs
```
