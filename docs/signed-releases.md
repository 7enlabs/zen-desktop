# Signed Releases (R15)

Every ZenCode Desktop release is cryptographically signed with
[minisign](https://jedisct1.github.io/minisign/) (Ed25519). Users can verify
that a downloaded artifact was built by the ZenCode team and was not tampered
with in transit.

## Trust model

- **One release keypair** for the whole project. The **secret key** is held
  by Z App (project lead) outside the repository. The **public key** is
  committed as `docs/keys/zencode-release.pub`, embedded in
  `desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`, as the base64
  of the whole `.pub` file - the format Tauri expects) and mirrored below.
- The Tauri auto-updater **refuses to install** any update whose signature
  does not verify against the embedded public key. There is no bypass.
- Git tags are informational only; the signature on the artifact is what
  matters.

> **Status (2026-10-18):** the release keypair has been generated
> (key id `A459EE7B787B86FF`) and the public key is wired. The engine test
> `release_signing::tests::release_key_is_wired` fails the build if
> `tauri.conf.json` and `docs/keys/zencode-release.pub` ever disagree or the
> key is missing/placeholder. There is no separate CI job for this yet.
>
> The secret key was generated **without a password** (`minisign -G -W`),
> because the release machine has no interactive prompt. It is stored with
> mode 600 outside the repo plus one backup. Encrypting it (or moving it to a
> hardware token) before the first public release is recommended.

## Release public key

Key id `A459EE7B787B86FF` (file: `docs/keys/zencode-release.pub`):

```
untrusted comment: minisign public key A459EE7B787B86FF
RWT/hnt4e+5ZpJXAoEvIN4/C6zd6KY21MSADXo6RXLx9eayno0OMrPfu
```

## Signing a release (maintainer)

Prerequisites: the release secret key (`zencode-release.key`), `minisign`
installed, artifacts already built (`.AppImage`, `.dmg`, `.msi`/`.exe`,
`latest.json`).

```bash
# From the repo root. Signs every artifact in dist/ and writes dist/SHA256SUMS.
MINISIGN_KEY=/secure/path/zencode-release.key tools/sign-release.sh v0.4.0 dist/
```

What the script does:
1. Refuses to run if `MINISIGN_KEY` is unset or the file does not exist.
2. Refuses to run if `tauri.conf.json` or `docs/keys/zencode-release.pub` is
   missing, or the updater pubkey is still a `PLACEHOLDER`.
3. For each artifact file: `minisign -S -x <file>.minisig -s $MINISIGN_KEY -m <file>`
   (trusted comment records the version and filename), then re-verifies the
   new signature against the published `docs/keys/zencode-release.pub`, so a
   wrong secret key fails the run instead of shipping unverifiable artifacts.
4. Writes `SHA256SUMS` and signs that too (`SHA256SUMS.minisig`), so a single
   signature covers the whole release.

The signed files (`*.minisig`, `SHA256SUMS`, `SHA256SUMS.minisig`,
`latest.json`) are uploaded to the release page.

## Updater releases in CI (0.3-4)

Updater artifacts are signed ONLY in GitLab CI. The key is stored as the
masked, protected CI variables `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, so it is only exposed to pipelines for
protected branches/tags. Pushing a protected tag `vX.Y.Z` (equal to the
`version` in `tauri.conf.json`) runs `release-linux` (`tools/release-linux.sh`)
and `release-publish` in `.gitlab-ci.yml`:

1. `cargo tauri build --bundles appimage`.
2. The AppImage is copied to `zen-desktop_<version>_<arch>.AppImage` and
   signed with `cargo tauri signer sign`, so the signed trusted comment is
   `timestamp:<unix>\tfile:zen-desktop_<version>_<arch>.AppImage`.
3. `latest.json` (platform `linux-x86_64`) is written, both files go to the
   generic package registry, and the GitLab release links `latest.json` as
   `/latest.json`, which backs the only updater endpoint:
   `https://gitlab.com/7enlab/zen-desktop/-/releases/permalink/latest/downloads/latest.json`.

**Signed-version check.** The updater plugin always verifies the artifact
signature, but the manifest `version` is unsigned. The app therefore installs a
`default_version_comparator` that calls
`zencode_engine::release_signing::check_update_offer`: the offered version must
be newer than the running one, must appear as a `_`-separated token of the
signed file name, and the download URL must point at that same file. A replayed
old signature or a relabeled manifest is refused before download. macOS and
Windows CI release jobs are not set up yet.

## Verifying a download (user)

```bash
# 1. Get the release public key (from this file or the website).
# 2. Verify:
tools/verify-release.sh docs/keys/zencode-release.pub ZenCode-0.4.0.AppImage
```

The script checks the `.minisig` next to the file and prints
`Signature verified` on success. Any mismatch (wrong key, modified file)
fails loudly.

Manual equivalent:

```bash
minisign -V -p docs/keys/zencode-release.pub -m ZenCode-0.4.0.AppImage
```

## Updater integration

- `desktop/src-tauri/tauri.conf.json` → `plugins.updater`:
  - `pubkey`: base64 of the whole `docs/keys/zencode-release.pub` file
    (Tauri format; a bare `RW...` line is NOT accepted).
  - `endpoints`: release metadata URLs. **Must point at GitLab**
    (`https://gitlab.com/7enlab/zen-desktop/-/releases/...`) while the
    GitHub account is suspended; GitHub endpoints are kept as fallback only.
  - `active: true`, `dialog: true` - the user always sees and confirms updates.
- `desktop/src-tauri/src/main.rs` registers `tauri_plugin_updater`. The plugin
  verifies the Ed25519 signature of the downloaded bundle **before**
  installation; a bad signature aborts the update with an error dialog.
- `engine/src/release_signing.rs` (this crate) contains the offline
  verification logic used by tests and by any future non-Tauri install path:
  `verify_file(public_key, file)` → `Ok(())` / `Err(VerifyError)`;
  `verify_bytes_with_comment` returns the verified trusted comment so a caller
  can bind it to the expected version/file name (anti-downgrade);
  `updater_public_key(tauri_conf)` extracts and validates the embedded key.
  It uses `minisign-verify` (zero dependencies, verification only - the same
  crate `tauri-plugin-updater` uses) and rejects legacy non-prehashed
  signatures.

## Key rotation

If the secret key is ever suspected compromised:
1. Z App generates a new keypair offline.
2. The new public key ships in the **next** release (signed with the **old**
   key, so the updater trusts it).
3. `docs/signed-releases.md` records the rotation with dates; the old public
   key is kept in the "Retired keys" section for verifying old artifacts.

### Retired keys

(none yet)

## Files

| File | Purpose |
|---|---|
| `tools/sign-release.sh` | Maintainer: sign all artifacts for a release |
| `tools/verify-release.sh` | User/CI: verify one artifact against a public key |
| `engine/src/release_signing.rs` | Rust verification logic + tests (test keypair only) |
| `docs/keys/zencode-release.pub` | Release public key (minisign) |
| `desktop/src-tauri/tauri.conf.json` | Updater `pubkey` + `endpoints` |
