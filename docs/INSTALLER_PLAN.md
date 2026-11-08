# Windows Installer Plan

## Scope

Prepare the first Windows installer for the existing native GPUI application. This is the first installer distribution, not the project's first public release: `CHANGELOG.md` identifies `v0.1.0` as the first public release, and earlier tags must not be moved or overwritten. The first installer was prepared for `v0.1.44`; the plan is updated through the `v0.1.57` installer.

Initial target: x64 Windows on MSVC. Keep the installer native and install the desktop application plus the standalone `dshell-cli.exe` command tool. Do not add a WebView runtime, background service, auto-start entry, telemetry, or file associations.

## Installer Choice

Use Inno Setup 6.7.3 for the first installer. The current product is a single native desktop application and needs a compact setup flow, per-user installation, Start Menu shortcuts, upgrade handling, and uninstall behavior. Inno Setup provides a script-controlled Windows Setup executable, configurable wizard appearance and icons, shortcuts, and uninstall support.

Inno Setup 6.7.3 is selected for this build. The project owner confirmed non-commercial open-source use, satisfying the builder-license review for this release.

Keep MSIX as a later option for Microsoft Store or managed enterprise distribution. MSIX offers package identity and clean package lifecycle behavior, but it requires a valid trusted package signature; the signing and publisher identity are not currently configured. Do not build both formats for the first installer milestone.

## Icon and Visual Treatment

- Reuse `assets/dshell-icon.svg` as the source mark and `assets/dshell.ico` for the installer, uninstaller, app executable, and shortcuts.
- Reuse the terminal mark in the wizard's small branding image. Keep the wizard layout conventional and content-first; use the `modern` built-in style with Windows-controlled light/dark behavior rather than a custom full-screen splash or a non-native installer UI.
- Include light and dark wizard image variants only if the first isolated Windows review finds the single mark loses contrast in one mode.
- Keep the app title, installer title, Start Menu folder, and uninstall display name consistent as `dshell`.

## Wizard Flow

1. Welcome: identify dshell as a native SSH client and show the app icon.
2. License: present the repository MIT license.
3. Install location: default to the current user's local applications folder and avoid elevation by default. Do not silently switch to an all-users installation.
4. Start Menu: create a dshell folder and application shortcut.
5. Additional tasks: offer an unchecked Desktop shortcut. Do not offer startup launch.
6. Ready: summarize the destination and selected shortcut options.
7. Installing: show normal file-copy progress and support cancellation before finalization.
8. Finished: offer “Launch dshell” as an unchecked option.

The app has no selectable feature components, so omit a feature-selection page. Do not make internet access a setup requirement.

## Upgrade and Uninstall

- Use one stable Inno Setup `AppId` across releases so a new installer upgrades the existing installation instead of creating duplicate entries.
- Preserve `%APPDATA%\\dshell` host profiles, settings, keys, logs, and other user data on upgrade and uninstall. The first installer must never delete user data implicitly.
- Keep configuration and SSH data outside the installation directory. Verify upgrades when the app is running and show a clear close-app prompt if files are locked.
- The installer must not change `known_hosts`, create SSH credentials, or alter credential persistence behavior.

## Release Artifacts and Signing

- Build the release executable with `cargo build --release --locked`; package the exact executable and required runtime files after a clean-machine dependency check.
- Produce `dshell-setup-v0.1.52-x64.exe`, plus a SHA-256 checksum and source tag reference.
- Sign the application executable and setup executable with Authenticode and timestamp them if a verified publisher certificate is available. If signing is unavailable, make that visible in the release decision and notes rather than claiming a signed installer.
- Keep a portable ZIP as a separate optional artifact; it must not be required by the installer workflow.

## Acceptance Checks

- Install as a standard user on a clean supported Windows VM without an elevated prompt.
- Run `scripts\\test-installer.ps1` for a silent temporary-directory install/uninstall smoke test before isolated GUI testing.
- Verify the icon in the Setup executable, installed executable, Start Menu shortcut, taskbar, and uninstall entry.
- Verify first install, same-version repair, upgrade, cancellation, and uninstall.
- Confirm host profiles, settings, SSH keys, logs, and `known_hosts` remain unchanged by installation and are preserved on uninstall.
- Confirm the installer has no network dependency, creates only the documented shortcuts, and does not launch dshell unless selected on the Finished page.
- Run the release baseline: `cargo fmt --all`, `scripts\\verify-hard-constraints.ps1`, locked all-target checking, unit tests, and Docker SSH integration tests; record test counts, Docker results, signing state, and isolated Windows GUI checks in `CHANGELOG.md` and `IMPLEMENTATION_STATUS.md`.

## Decisions Before Implementation

- Release scope is frozen as `v0.1.52`; earlier tags are not replaced.
- First release supports x64 per-user installation only.
- No Authenticode certificate is available for this candidate; the installer and EXE are unsigned. Decide whether to add a signing certificate before public publication.
- The optional portable ZIP is not included in this first installer candidate.

## References

- Inno Setup official help: “What is Inno Setup?”, `[Setup]`, `WizardStyle`, and `[Icons]`.
- Microsoft Learn: “What is MSIX?” and “Sign an MSIX package”.
