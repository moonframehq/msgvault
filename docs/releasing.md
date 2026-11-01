# Releasing

## Workflows

| Workflow | Runs on | Does |
| --- | --- | --- |
| `ci.yml` | every pull request, pushes to `main` | zizmor on the workflows; formatting; `Cargo.lock`, clippy, tests and the app's headless `--check` on macOS |
| `package.yml` | pull requests touching packaging, each release | optimized builds and the unsigned packages for every platform, each checked headlessly against the app compiled into it |
| `supply-chain.yml` | dependency changes, weekly | cargo-deny: advisories, bans, licenses, sources |
| `release.yml` | `just release` | the release, below |

Every action is pinned to a commit SHA (Dependabot moves them, after a
seven-day cooldown), every job declares the permissions it needs and no
more, checkouts never keep credentials, and no job that feeds a release uses
a cache. `just audit` runs the same zizmor, actionlint and cargo-deny checks
locally.

## Making a release

1. Set `version` under `[workspace.package]` in `Cargo.toml` and merge it to
   `main`. The release is tagged `v<version>` and refuses a version that
   already has a tag or a release.
2. On an up-to-date, clean checkout of `main`, run `just release`.

The workflow then:

1. runs zizmor on itself and checks the requested commit is `main`'s HEAD;
2. builds every package (`package.yml`) and the CycloneDX SBOMs, while it
   waits for CI to pass on that same commit; nothing below runs until it has;
3. signs msgvault.app with the Developer ID, notarizes and staples it, and builds
   the disk image around it, signed, notarized and stapled too (required:
   without a Developer ID the release fails, see below); and signs the
   Windows executable with Authenticode when its signing is set up;
4. writes `SHA256SUMS`, signs every file with Sigstore (keyless, as this
   workflow), and records GitHub attestations of SLSA build provenance and
   of the SBOM for every file;
5. verifies all of that as a user would, creates the tag and a draft release
   with generated notes, checks the uploaded files against `SHA256SUMS`, and
   publishes it.

Only the `sign-macos`, `sign-windows`, `attest` and `publish` jobs enter
the `release` environment. Protect it (Settings → Environments) so it only accepts
`main`, and add yourself as a required reviewer if releases should wait for
an approval.

## What a release contains

| File | For |
| --- | --- |
| `msgvault-moonframe-<v>-universal-apple-darwin.dmg` | macOS 13+, Apple Silicon and Intel: drag msgvault onto the Applications shortcut |
| `msgvault-moonframe-<v>-{x86_64,aarch64}-unknown-linux-gnu.tar.gz` | Linux: run `msgvault-moonframe` from the unpacked folder |
| `msgvault-moonframe-<v>-x86_64-pc-windows-msvc.zip` | Windows: run `msgvault-moonframe.exe` from the unpacked folder |
| `msgvault-moonframe-<v>.cdx.json` | the SBOM of the executable, Moonframe included |
| `SHA256SUMS` | the checksum of every file above |
| `*.sigstore.json` | the Sigstore bundle of each file |

Every executable carries its Luau app and Moonframe's toolbox inside it.
The macOS bundle also holds them in `Contents/Resources`, with the toolbox's
Wasm providers prebuilt; the Linux and Windows archives build none, as the
app only runs native providers.

These names are what the app's **Check for Updates…** looks for
(Moonframe's updater, `.updates("penso/msgvault-moonframe")` in `main.rs`):
an installed copy checks the newest published release once a day. On macOS
it installs the disk image after checking it against `SHA256SUMS`, its
bundle id and, when signed with a Developer ID, its team; elsewhere it opens
the download. Drafts and pre-releases are never offered.

## Verifying a download

```sh
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify msgvault-moonframe-<v>-universal-apple-darwin.dmg --repo penso/msgvault-moonframe
cosign verify-blob msgvault-moonframe-<v>-universal-apple-darwin.dmg \
  --bundle msgvault-moonframe-<v>-universal-apple-darwin.dmg.sigstore.json \
  --certificate-identity https://github.com/penso/msgvault-moonframe/.github/workflows/release.yml@refs/heads/main \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

## Setting up macOS signing

A release requires it: the workflow fails before building anything when
`MACOS_SIGNING_IDENTITY` is not a Developer ID Application identity. The app
installs its own updates and holds each one to the signing team of the
running copy; an app signed ad hoc names no team, so the copies installed
from it would accept an update signed by anyone. To sign and notarize, add
to the `release` environment:

| Name | Kind | Value |
| --- | --- | --- |
| `MACOS_SIGNING_IDENTITY` | variable | `Developer ID Application: Name (TEAMID)` |
| `MACOS_CERTIFICATE_P12_BASE64` | secret | the Developer ID Application certificate and key, exported as .p12, base64 |
| `MACOS_CERTIFICATE_PASSWORD` | secret | that .p12's password |
| `APPLE_API_KEY_ID` | variable | an App Store Connect API key's ID, for notarization |
| `APPLE_API_ISSUER_ID` | variable | its issuer ID |
| `APPLE_API_PRIVATE_KEY` | secret | its `.p8` contents |

`MACOS_SIGNING_IDENTITY` is read outside the environment too, to check it
before anything is built, so set it as a repository variable as well.

The app runs under the hardened runtime with the entitlements in
`assets/macos/msgvault.entitlements`: wasmtime compiles the toolbox's Wasm providers
to executable memory, which the hardened runtime otherwise refuses.

## Setting up Windows signing

Without it, the zip's `msgvault-moonframe.exe` is unsigned and SmartScreen warns on
first launch. It is signed with Azure Artifact Signing, under the publisher
`Fabien Penso`, by the account and certificate profile every one of the
author's apps shares. There is no key, certificate or secret to store: the
`sign-windows` job exchanges the `release` environment's GitHub OIDC token
for an Azure token as the `moonframe-signing` app registration, which needs
a federated credential trusting `repo:penso/msgvault-moonframe:environment:release`
(beside Moonframe's own) and holds the *Artifact Signing Certificate Profile Signer* role on
that profile. Add to the `release` environment, all as variables:

| Name | Value |
| --- | --- |
| `AZURE_CLIENT_ID` | the `moonframe-signing` app registration's application ID |
| `AZURE_TENANT_ID` | its tenant |
| `AZURE_SUBSCRIPTION_ID` | the subscription holding the signing account |
| `ARTIFACT_SIGNING_ENDPOINT` | the signing account's regional endpoint |
| `ARTIFACT_SIGNING_ACCOUNT` | the signing account |
| `ARTIFACT_SIGNING_PROFILE` | the certificate profile |

and, as a repository variable (setting it turns signing on),
`WINDOWS_SIGNING_SUBJECT`: the certificate's exact subject, which the job
requires of every signature, with a timestamp, before it zips the folder
again.

When the publisher's identity validation in Azure expires, signing fails
until it is renewed (Artifact Signing account → Identity validations).

## A public repository

The attestations (`actions/attest`) and Sigstore's public transparency log
need the repository to be public, as Moonframe's is; on a private one the
`attest` job fails and nothing is published.
