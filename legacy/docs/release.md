# Music Player release process

Music Player 1.0.0 uses self-contained x64 and ARM64 portable ZIPs, per-user setup
installers, and an MSIX bundle. The MSIX identity is `Kalabhaftu.MusicPlayer`,
publisher `CN=Kalabhaftu`. Setup embeds Microsoft's signed WebView2 Evergreen
bootstrapper. The managed runtime, WinUI and VLC modules remain included.

## Evidence required before release

Keep the readiness PR open until protected `Core tests · Linux`, `x64`, and
`ARM64` checks pass on its final revision. Windows CI includes native playback,
format, tag, video/subtitle and WebView interaction tests. Dispatch **Signed
Windows release**, mode **candidate**, on the candidate branch to create private
artifacts and test signed setup/MSIX installation, upgrade, uninstall, file
associations, both data-retention choices, package size and startup performance.
A passing build alone does not complete these gates. Record revision, commands
and actual results in [project-status.md](project-status.md) and
[optimization-validation.md](optimization-validation.md).

Review unique backup-branch content and verify a full Git bundle outside the
checkout before deleting the local backup branch. Merge the verified PR using
GitHub's protected squash merge, synchronize local `main`, and verify the merge
commit's REST API `commit.verification.verified` value, `reason: valid`, and
`committer.login: web-flow`. This confirms the commit was created on GitHub.com
and signed with GitHub's verified signature, as requested.
GitHub's verified commit signature authenticates the source revision. Windows
package signing uses the separate project certificate described below.

Run private signing preflight and repeat full candidate package validation on
merged `main`. Only after every gate passes, create `v1.0.0` at that verified
commit. The workflow rejects tags outside `main` and unverified final commits.

## Persistent self-signing

The selected mode is explicitly **self-signed**, not publicly trusted Windows
publisher signing. Reuse the persistent RSA/SHA-256 Code Signing certificate
with subject `CN=Kalabhaftu` across builds and upgrades. Protect its PFX and
password outside the repository. Configure `MUSICPLAYER_SIGNING_PFX_BASE64` and
`MUSICPLAYER_SIGNING_PFX_PASSWORD` Actions secrets through stdin without exposing
values. The public certificate's SHA-256 fingerprint is:

```text
91BB3E5022E52803329F928B8CCAD73E3211AE710B764B373480A2FB1C5EDEE4
```

Dispatch **Signed Windows release**, mode **preflight**, on `main`. It checks
the verified source commit, certificate identity, fingerprint, validity, signing
purpose, private key, timestamped executable signing and detached CMS signing.
Trust-dependent checks import the certificate only into the disposable runner.
Preflight publishes nothing. Logs suppress private signing material. Missing
secrets or failed validation stop packaging.

Packaging signs first-party executables, assemblies, shipped PowerShell helpers,
setup and its embedded uninstaller, and MSIX packages, preserving vendor
signatures. Authenticode/MSIX signatures receive timestamps. ZIP bytes are
covered by the authenticated SHA-256 checksum manifest.

## Installation and saved data

Setup installs for the current account under `%LOCALAPPDATA%\Programs\Music
Player`. It registers audio-file actions, removes them on uninstall, and defaults
to keeping `%LOCALAPPDATA%\MusicPlayer` data. Removing app data never removes
source music. Portable copies include registration and uninstall helpers.
Windows requires the user to choose a default player explicitly.

MSIX requires an intentional certificate-trust step. Compare the downloaded
public certificate's fingerprint with independently obtained repository
instructions, then follow [SIGNING.md](../packaging/windows/SIGNING.md) to import
it into **Trusted People**. No installer automatically changes trust on a user's
PC. Publisher and SmartScreen warnings remain expected with self-signing.
Microsoft describes the [MSIX self-signed distribution requirements](https://learn.microsoft.com/en-us/windows/msix/package/sign-msix-package-guide).

Windows normally removes MSIX private data during uninstall. The signed
`Uninstall-MusicPlayer-MSIX.ps1` helper offers retention outside the package or
explicit removal. Existing recovery copies and source music are preserved.
Both choices must pass isolated package tests.

## Download verification and publication

The manifest covers both portable ZIPs, both setup installers, the MSIX bundle,
third-party notices, public certificate, signing instructions, checksum verifier,
and MSIX uninstall helper. The `.txt.p7s` file authenticates the exact checksum
manifest with the persistent project key. Put all assets in one directory and
run an independently obtained verifier:

```powershell
pwsh -NoProfile -File .\Verify-MusicPlayer-ReleaseManifest.ps1 -ManifestPath .\MusicPlayer-1.0.0-SHA256SUMS.txt
```

The verifier checks the CMS signature, pinned certificate, validity, signing
purpose, exact asset set and every hash. It does not establish public CA trust
or online revocation. The detached manifest is not timestamped and requires
the project certificate to remain valid.

After build, installation and performance gates pass, the workflow uploads a
**draft** release, downloads every asset, compares its bytes with the validated
candidate and repeats manifest authentication. Only then does it publish the
release. A verification failure leaves the draft unpublished. Published assets
are immutable; corrections require a new version. Verify the final downloaded
assets and workflow result after publication.
