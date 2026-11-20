# Design: Windows Public Installer for AfterTone (`AfterTone-Setup.exe`)

## 1. Intent & Context
Users need an easy, one-click Windows installation setup (`AfterTone-Setup.exe`) that distributes the native player, its Compose Desktop UI runtime, audio proxy, and client assets without exposing the user to git repositories, terminal commands, or directory hierarchies.

---

## 2. Goals, Non-Goals, and Anti-Goals

### Goals
- Produce a single, standalone `AfterTone-Setup.exe` that any Windows user can download and double-click to install.
- Deploy the complete runtime package (`aftertone-native.exe`, `app/`, `runtime/`, `client/`, `WebView2Loader.dll`, `AfterTone.ico`).
- Create an **AfterTone** shortcut in the Windows Start Menu programs list with the branded icon.
- Provide an optional Desktop shortcut toggle during installation.
- Support clean uninstallation via Windows Settings &rarr; Installed Apps.
- Default to per-user installation (`%LOCALAPPDATA%\Programs\AfterTone`) so standard users without administrator privileges can install without UAC elevation prompts.
- Provide an automated build script (`scripts/build-installer.ps1`) that compiles the installer reproducible from source.

### Non-Goals
- Microsoft Store MSIX packaging (can be pursued in a later release).
- Network streaming web installers (the full offline installer is preferred for immediate playback).

### Anti-Goals
- Requiring administrative credentials or UAC prompts for standard users.
- Leaving orphan files in temp directories or user documents.

---

## 3. Approaches Considered

1. **Approach 1: Inno Setup (`AfterTone-Setup.exe`) — Selected**
   - High-ratio LZMA2 compression (reducing ~677 MB uncompressed payload to ~250 MB).
   - Standard Windows wizard with license/info, destination picker, shortcut toggles, and launch-on-finish.
   - Per-user installation support (`PrivilegesRequired=lowest`), built-in uninstaller, and Start Menu registration.
2. **Approach 2: Custom Rust SFX Binary**
   - Rejected: Re-invents Windows shell shortcut creation and uninstaller registry entries with more maintenance overhead.
3. **Approach 3: 7-Zip SFX Module**
   - Rejected: Lacks native Windows wizard UI and clean Add/Remove Programs integration.

---

## 4. Technical Specification

### Metadata
- **App Name:** AfterTone
- **App Version:** 1.0.0
- **App Publisher:** AfterTone Contributors (`https://github.com/arzeck/AfterTone`)
- **App ID:** `{{E57D549B-5154-4C78-984F-66D67098DAE1}}`
- **Output Executable:** `target/dist/AfterTone-Setup-v1.0.0.exe`

### File Layout in Target Installation Directory (`{app}`)
```text
{app}/
├── aftertone-native.exe       # Main executable
├── WebView2Loader.dll         # WebView2 dependency
├── AfterTone.ico              # Multi-res application icon
├── app/                       # Compose Desktop JARs and Skiko natives
├── runtime/                   # Bundled Java 21 JRE
└── client/                    # Built web client assets
```

### Shortcuts & Registration
- **Start Menu:** `{autoprograms}\AfterTone\AfterTone.lnk` &rarr; `{app}\aftertone-native.exe` (Icon: `{app}\AfterTone.ico`)
- **Desktop (Optional):** `{autodesktop}\AfterTone.lnk` &rarr; `{app}\aftertone-native.exe`
- **Add/Remove Programs:** Display Name: `AfterTone`, Version: `1.0.0`, Publisher: `AfterTone Contributors`, Display Icon: `{app}\AfterTone.ico`.

### Automated Build Pipeline
- File: `scripts/build-installer.ps1`
  - Validates `aftertone-native.exe` release binary exists (or triggers `cargo build --release`).
  - Checks if Inno Setup (`ISCC.exe`) is installed; if not, offers/installs via `winget install JRSoftware.InnoSetup --source winget` or locates from standard install paths (`C:\Program Files (x86)\Inno Setup 6\ISCC.exe`).
  - Compiles `installer/aftertone.iss` and outputs `target/dist/AfterTone-Setup-v1.0.0.exe`.
