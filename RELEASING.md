# Releasing

## Signing keys

Release tags from 0.1.0 on are signed with the SSH key whose public part is
in [docs/chipclean-release.pub](docs/chipclean-release.pub).

To verify a tag:

```sh
git cat-file tag v0.1.0
git verify-tag v0.1.0   # after adding the pubkey to allowed signers
```

## Create a release

1. Update `CHANGELOG.md` (move Unreleased → new version).
2. Bump `version` in `Cargo.toml` if needed.
3. Commit: `git commit -m "chore(release): 0.1.0"`.
4. Tag (signed):

```sh
git tag -s v0.1.0 -m "ChipClean 0.1.0"
git push origin v0.1.0
```

5. The `release.yml` workflow builds `chipclean.exe` on Windows, writes
   `SHA256SUMS.txt`, and creates the GitHub Release with artifacts.
6. Verify the workflow result and the attached checksums.

## Rules

- Never push tags without a signed release tag.
- Do not attach registry data or personal files to releases.
