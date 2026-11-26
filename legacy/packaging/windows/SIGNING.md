# Music Player self-signed distribution

Music Player 1.0.0 uses a persistent, self-signed RSA/SHA-256 code-signing
certificate with subject `CN=Kalabhaftu`. It is **not publicly trusted signing**.
Windows may show “Unknown publisher” or SmartScreen warnings. A signature proves
integrity against the project key; it does not independently verify the developer's
identity or guarantee that software is safe.

Certificate SHA-256 fingerprint:

`91BB3E5022E52803329F928B8CCAD73E3211AE710B764B373480A2FB1C5EDEE4`

Get this fingerprint and the verification script from
https://github.com/kalabhaftu/music-player over HTTPS independently of your download.
Run `Verify-MusicPlayer-ReleaseManifest.ps1 -ManifestPath
MusicPlayer-1.0.0-SHA256SUMS.txt` with all release assets in the same directory.
The script checks the detached signature, the pinned project certificate, and
every listed asset hash without adding certificate trust to your computer.

Portable and setup packages are self-contained. A Windows warning may require
your explicit decision to run them. They do not install the certificate for you.

To install the MSIX bundle, first inspect `MusicPlayer-Signing.cer` in Windows
and compare its SHA-256 fingerprint. Only if you choose to trust this publisher,
install that **public certificate** into Local Machine → Trusted People using
Windows Certificate Import Wizard (administrator permission may be needed).
Do not put it in Trusted Root Certification Authorities. Then install the MSIX
bundle. See Microsoft's requirements:
https://learn.microsoft.com/en-us/windows/msix/package/sign-msix-package-guide

To revoke that local choice, remove the matching certificate from Trusted People.
Keep the same certificate for upgrades. The private key and password are never
distributed; only the `.cer` is public.
# MSIX uninstall and saved data

Windows' normal MSIX uninstall removes the package's private data. To retain your
library, close Music Player and run the downloaded, signed
`Uninstall-MusicPlayer-MSIX.ps1` helper without arguments. It copies the saved
profile to `%LOCALAPPDATA%\MusicPlayer` before removing the package, so reinstall
can read it. If an unpackaged profile already exists, it first preserves a copy
under `%LOCALAPPDATA%\MusicPlayerUninstallBackups`.

Use `-RemoveUserData` to remove the saved profile along with MSIX. Source music and
existing recovery backups are preserved. This helper never installs certificate
trust. Authenticate it with the release checksum manifest before running it.
