# Windows Public Installer (`AfterTone-Setup.exe`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create an automated, production-ready Windows installer (`AfterTone-Setup-v1.0.0.exe`) using Inno Setup that installs the complete AfterTone native player, creates Start Menu and Desktop shortcuts with custom branding, registers an uninstaller, and enables single-click deployment for end users.

**Architecture:** An Inno Setup definition script (`windows/installer/aftertone.iss`) defines the packaging layout, shortcuts, per-user default directory (`%LOCALAPPDATA%\Programs\AfterTone`), and uninstallation hooks with LZMA2 solid compression. A PowerShell orchestration script (`windows/scripts/build-installer.ps1`) validates release binaries, installs/locates the Inno Setup compiler (`ISCC.exe`), and builds the output executable into `windows/dist/`.

**Tech Stack:** Inno Setup 6 (`ISCC.exe`), PowerShell 5.1/7+, Windows Shell Shortcut API (`.lnk`), Cargo / Rust.

**Spec:** [docs/superpowers/specs/2026-11-21-windows-installer-design.md](file:///c:/Users/Aneesh/Documents/Projects/aftertone-native/docs/superpowers/specs/2026-11-21-windows-installer-design.md)

## Global Constraints

- App Name: `AfterTone`
- App Version: `1.0.0`
- App Publisher: `AfterTone Contributors`
- GitHub Repo URL: `https://github.com/arzeck/AfterTone`
- App ID: `{{E57D549B-5154-4C78-984F-66D67098DAE1}}`
- Target Default Directory: `{autopf}\AfterTone` (`PrivilegesRequired=lowest` for non-admin user installs)
- Start Menu Shortcut Path: `{autoprograms}\AfterTone\AfterTone.lnk`
- Working Directory: `{app}`
- Output Directory: `target/dist/`
- Output Setup Filename: `AfterTone-Setup-v1.0.0.exe`

## Review Focus

1. **Missing release binary:** If `engine/target/release/aftertone-native.exe` is missing when building the installer, `windows/scripts/build-installer.ps1` must detect it and trigger `cargo build --release` automatically.
2. **Missing Inno Setup compiler:** If `ISCC.exe` is not in PATH or Program Files, the build script must install it via `winget` or guide the user without crashing.
3. **Working directory on shortcut:** The Start Menu shortcut must set `WorkingDir: "{app}"` so Java runtime and Compose Skiko libraries resolve properly relative to the exe.
4. **Clean uninstallation:** The uninstaller must clean up all installed files, shortcuts, and registry keys without deleting user personal files.
5. **No UAC block for standard users:** `PrivilegesRequired=lowest` must allow installation in corporate/restricted environments where standard users do not have local administrator rights.

---

### Task 1: Inno Setup Configuration Script (`windows/installer/aftertone.iss`)

**Files:**
- Create: `windows/installer/aftertone.iss`

**Interfaces:**
- Consumes: `engine/target/release/aftertone-native.exe`, `windows/app/*`, `windows/runtime/*`, `web/*`, `windows/WebView2Loader.dll`, `windows/app/AfterTone.ico`
- Produces: `windows/dist/AfterTone-Setup-v1.0.0.exe` when compiled with `ISCC.exe`

- [ ] **Step 1: Create `windows/installer/aftertone.iss` with complete metadata and directory layout**
  Define:
  - `[Setup]` block with `AppId`, `AppName`, `AppVersion`, `AppPublisher`, `AppPublisherURL`, `DefaultDirName={autopf}\AfterTone`, `DefaultGroupName=AfterTone`, `PrivilegesRequired=lowest`, `OutputDir=..\dist`, `OutputBaseFilename=AfterTone-Setup-v1.0.0`, `SetupIconFile=..\app\AfterTone.ico`, `Compression=lzma2/ultra64`, `SolidCompression=yes`.
  - `[Tasks]` block for desktop shortcut toggle (`desktopicon`).
  - `[Files]` block packaging:
    - `..\..\engine\target\release\aftertone-native.exe` &rarr; `{app}`
    - `..\WebView2Loader.dll` &rarr; `{app}`
    - `..\app\AfterTone.ico` &rarr; `{app}`
    - `..\app\*` &rarr; `{app}\app` (recursive)
    - `..\runtime\*` &rarr; `{app}\runtime` (recursive)
    - `..\..\web\*` &rarr; `{app}\web` (recursive)
  - `[Icons]` block creating:
    - `{autoprograms}\AfterTone\AfterTone.lnk` (Target: `{app}\aftertone-native.exe`, WorkingDir: `{app}`, Icon: `{app}\AfterTone.ico`)
    - `{autodesktop}\AfterTone.lnk` (Target: `{app}\aftertone-native.exe`, WorkingDir: `{app}`, Icon: `{app}\AfterTone.ico`, Tasks: `desktopicon`)
  - `[Run]` block with `postinstall nowait skipifsilent` checkbox to launch AfterTone.

- [ ] **Step 2: Verify `windows/installer/aftertone.iss` syntax**
  Verify file content and syntax matching Inno Setup 6 specifications.

---

### Task 2: Build Orchestration Script (`windows/scripts/build-installer.ps1`)

**Files:**
- Create: `windows/scripts/build-installer.ps1`

**Interfaces:**
- Consumes: Inno Setup script `windows/installer/aftertone.iss` and release payload directories
- Produces: Executable packaging flow

- [ ] **Step 1: Write `windows/scripts/build-installer.ps1`**
  Implement:
  1. Check for `aftertone-native.exe`; run `cargo build --release` if missing.
  2. Locate `ISCC.exe`:
     - Test `Get-Command ISCC -ErrorAction SilentlyContinue`
     - Test `C:\Program Files (x86)\Inno Setup 6\ISCC.exe`
     - Test `C:\Program Files\Inno Setup 6\ISCC.exe`
     - If not found, attempt install via `winget install --id JRSoftware.InnoSetup -e --source winget --accept-source-agreements --accept-package-agreements`.
  3. Ensure `windows/dist` directory exists.
  4. Run `& $isccPath "windows\installer\aftertone.iss"`.
  5. Print resulting file path and file size upon success.

- [ ] **Step 2: Test script execution on Windows**
  Execute `powershell -ExecutionPolicy Bypass -File windows/scripts/build-installer.ps1`.
  Verify Inno Setup compiler is found or installed, and compilation starts.

---

### Task 3: Build & Verification

**Files:**
- Output: `target/dist/AfterTone-Setup-v1.0.0.exe`

- [ ] **Step 1: Compile the installer**
  Run the automated script to produce the installer.

- [ ] **Step 2: Verify installer artifact**
  Check exit code `0`, verify `target/dist/AfterTone-Setup-v1.0.0.exe` exists, check file size (~240-280 MB), and inspect metadata.
