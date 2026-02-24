---
title: Releasing
weight: 7
---

## Release process

1. **Ensure clean state** — all quality gates must pass (see [CLAUDE.md](CLAUDE.md))

2. **Bump version, commit, tag, and push**

   ```bash
   cargo release patch --execute   # or minor/major
   ```

   This bumps `Cargo.toml`, creates a commit (`chore: Release voicsh version X.Y.Z`), tags `vX.Y.Z`, pushes both, and publishes to crates.io.

3. **Create GitHub release**

   ```bash
   gh release create vX.Y.Z --title "vX.Y.Z -- Short tagline" --notes-file -
   ```

   Write release notes with: highlights, install/upgrade command, and a changelog link to the compare view.
