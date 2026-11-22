# AfterTone Android Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build the standalone Android version of AfterTone under `android/` using BitChord's multiplatform Android architecture (`app`, `shared`, `sharedUi`, `native`) as the base, rebranded to `com.arzeck.aftertone`, with adaptive icons from `Logo.png`, Rust engine integration via JNI, and zero upstream telemetry or donation links.

**Architecture:** The Android application resides in `android/` structured as a multi-module Gradle project (`:app`, `:shared`, `:sharedUi`, `:native`). The Android UI is built with Compose Multiplatform with Material 3 and Haze glassmorphic effects, backed by ExoPlayer for audio playback. Business logic and data models reside in `:shared`, DSP routines for Automix reside in `:native` (C++ via CMake), and high-performance audio resolution, DES-ECB decryption, TTML parsing, and party server are bridged from AfterTone's Rust engine (`engine/`) via JNI (`cdylib`).

**Tech Stack:** Kotlin 2.3.20, Android Gradle Plugin (AGP) 8.10.1, Compose Multiplatform 1.10.3, AndroidX Media3 (ExoPlayer), C++17 (CMake 3.22.1), Rust 2021 (JNI 0.21, Axum, Tokio), Coroutines, Ktor, OkHttp.

**Spec:** `docs/superpowers/specs/2026-11-22-android-client-design.md`

## Global Constraints

- Root Android location: Strictly in `android/` at repo root.
- Git branch: Strictly on `feat/android` (never touch `main`).
- Application ID: `com.arzeck.aftertone` (prod), `com.arzeck.aftertone.dev` (dev).
- Package namespace: `com.arzeck.aftertone.*`.
- SDK levels: `compileSdk = 37`, `targetSdk = 36`, `minSdk = 26`.
- Upstream telemetry removal: Zero requests or pings to `api.bitchord.kushagrasingh.in`.
- Upstream donation removal: No Ko-fi, PayPal, or UPI payment links.
- Licensing & Attribution: Maintain GPL-3.0 headers and acknowledge Kushagra Singh and the BitChord project in `NOTICE` and `README.md`.
- Platform support: Scripts and build commands must support Windows PowerShell and `gradlew.bat`.

## Review Focus

1. **Package renaming consistency:** Verify no stale `com.music.bitchord` imports or reflection strings remain in ported Kotlin/Java/XML files.
2. **Telemetry leakage:** Verify grep scans show zero occurrences of `api.bitchord.kushagrasingh.in` or tracking endpoints.
3. **Rust JNI symbol alignment:** Verify JNI export signatures in `engine/src/jni.rs` match `com.arzeck.aftertone.engine.AfterToneNative` exactly.
4. **CMake source path resolution:** Verify `CMakeLists.txt` resolves `native/analyzer` sources correctly relative to `android/native/`.
5. **Icon fidelity:** Ensure generated mipmap icons match `Logo.png` across all densities (`mdpi` through `xxxhdpi`).

---

### Task 1: Scaffold Android Root Build & Tooling Setup

**Files:**
- Create: `android/build.gradle.kts`
- Create: `android/settings.gradle.kts`
- Create: `android/gradle.properties`
- Create: `android/keystore.properties.example`
- Create: `android/.gitignore`
- Copy: `android/gradlew`, `android/gradlew.bat`, `android/gradle/wrapper/gradle-wrapper.jar`, `android/gradle/wrapper/gradle-wrapper.properties` from `C:\Users\Aneesh\Documents\Projects\BitChord`

- [x] **Step 1:** Create `android/` folder and copy Gradle wrapper binaries and scripts (`gradlew`, `gradlew.bat`, `gradle/`).
- [x] **Step 2:** Write `android/settings.gradle.kts` configuring `rootProject.name = "AfterTone"`, repositories (Google, Maven Central, JitPack), and subproject includes (`:shared`, `:sharedUi`, `:app`).
- [x] **Step 3:** Write `android/build.gradle.kts` with AGP 8.10.1, R8 8.13.23 classpath, Kotlin 2.3.20, and Compose 1.10.3 plugins.
- [x] **Step 4:** Write `android/gradle.properties` configuring AndroidX, JVM memory options, and non-transitive R classes.
- [x] **Step 5:** Write `android/keystore.properties.example` and `android/.gitignore` (ignoring `.gradle/`, `build/`, `local.properties`, `*.jks`).
- [x] **Step 6:** Verify Gradle wrapper execution: run `.\android\gradlew.bat --version` from PowerShell.
- [x] **Step 7:** Commit: `git commit -m "chore(android): scaffold root gradle build and wrapper"`

---

### Task 2: Port Native C++ Analyzer Module (`android/native/`)

**Files:**
- Create: `android/native/analyzer/audio_analysis.cpp`
- Create: `android/native/analyzer/audio_analysis.h`
- Create: `android/native/analyzer/mel_spectrogram.cpp`
- Create: `android/native/analyzer/mel_spectrogram.h`
- Create: `android/native/analyzer/resampler.cpp`
- Create: `android/native/analyzer/resampler.h`
- Create: `android/native/analyzer/tempo_analysis.cpp`
- Create: `android/native/analyzer/tempo_analysis.h`
- Create: `android/native/analyzer/vocal_spectrogram.cpp`
- Create: `android/native/analyzer/vocal_spectrogram.h`

- [x] **Step 1:** Copy `native/analyzer/` from upstream BitChord into `android/native/analyzer/`.
- [x] **Step 2:** Inspect headers and source files to verify standard C++17 compatibility with no broken relative includes.
- [x] **Step 3:** Verify all 10 source and header files exist and match upstream checksums.
- [x] **Step 4:** Commit: `git commit -m "feat(android): port native C++ audio analysis routines"`

---

### Task 3: Port & Rebrand `:shared` Kotlin Multiplatform Module (`android/shared/`)

**Files:**
- Create: `android/shared/build.gradle.kts`
- Create: `android/shared/src/commonMain/kotlin/com/arzeck/aftertone/...`
- Create: `android/shared/src/androidMain/kotlin/com/arzeck/aftertone/...`

- [x] **Step 1:** Copy `shared/` from BitChord into `android/shared/`.
- [x] **Step 2:** Rename directories and package declarations from `com.music.bitchord.*` to `com.arzeck.aftertone.*`.
- [x] **Step 3:** In `JamInvite.kt`, update invite link generation to `https://aftertone.app/invite/<CODE>` and deep link schema `aftertone://party/<CODE>`.
- [x] **Step 4:** Remove any calls or fallback URLs referencing `api.bitchord.kushagrasingh.in`.
- [x] **Step 5:** Verify `android/shared/build.gradle.kts` resolves dependencies and references correct namespace `com.arzeck.aftertone.shared`.
- [x] **Step 6:** Commit: `git commit -m "feat(android): port and rebrand shared data and domain module"`

---

### Task 4: Port & Rebrand `:sharedUi` Compose Multiplatform Module (`android/sharedUi/`)

**Files:**
- Create: `android/sharedUi/build.gradle.kts`
- Create: `android/sharedUi/src/commonMain/kotlin/com/arzeck/aftertone/sharedui/...`
- Create: `android/sharedUi/src/androidMain/kotlin/com/arzeck/aftertone/sharedui/...`

- [x] **Step 1:** Copy `sharedUi/` from BitChord into `android/sharedUi/`.
- [x] **Step 2:** Refactor package namespace from `com.music.bitchord.sharedui.*` to `com.arzeck.aftertone.sharedui.*`.
- [x] **Step 3:** Update imports across UI components (`NowPlayingScreen.kt`, `LyricsView.kt`, `Palette.kt`, etc.) to point to `com.arzeck.aftertone.*`.
- [x] **Step 4:** Verify `android/sharedUi/build.gradle.kts` references namespace `com.arzeck.aftertone.sharedui`.
- [x] **Step 5:** Commit: `git commit -m "feat(android): port and rebrand sharedUi Compose components"`

---

### Task 5: Port & Rebrand `:app` Module (`android/app/`)

**Files:**
- Create: `android/app/build.gradle.kts`
- Create: `android/app/src/main/AndroidManifest.xml`
- Create: `android/app/src/main/cpp/CMakeLists.txt`
- Create: `android/app/src/main/cpp/jni/*`
- Create: `android/app/src/main/java/com/arzeck/aftertone/...`
- Create: `android/app/src/main/res/...`

- [x] **Step 1:** Copy `app/` from BitChord into `android/app/`.
- [x] **Step 2:** In `android/app/build.gradle.kts`:
  - Set `namespace = "com.arzeck.aftertone"`.
  - Set `applicationId = "com.arzeck.aftertone"`.
  - Update dev flavor: `applicationId = "com.arzeck.aftertone.dev"`, `resValue("string", "app_name", "AfterTone Dev")`.
  - Update prod flavor: `resValue("string", "app_name", "AfterTone")`.
- [x] **Step 3:** Reorganize directory tree in `android/app/src/main/java/` from `com/music/bitchord/` to `com/arzeck/aftertone/`.
- [x] **Step 4:** Refactor package statements and imports across all Kotlin/Java files under `android/app/src/main/java/`.
- [x] **Step 5:** In `android/app/src/main/AndroidManifest.xml`, update package names, deep link schemes (`aftertone://`), and provider authorities.
- [x] **Step 6:** In `android/app/src/main/res/values/strings.xml`, change app title to "AfterTone" and remove BitChord strings.
- [x] **Step 7:** Remove donation links (Ko-fi, PayPal, UPI) and telemetry from Settings/About views.
- [x] **Step 8:** In `android/app/src/main/cpp/CMakeLists.txt`, ensure `BITCHORD_NATIVE_DIR` resolves cleanly to `android/native/` and outputs library `libbitchord_analysis.so` (or `libaftertone_analysis.so`).
- [x] **Step 9:** Commit: `git commit -m "feat(android): port and rebrand app module and playback service"`

---

### Task 6: Generate Adaptive and Legacy Android Icons from `Logo.png`

**Files:**
- Create: `android/app/src/main/res/mipmap-mdpi/ic_launcher.png` (48x48)
- Create: `android/app/src/main/res/mipmap-hdpi/ic_launcher.png` (72x72)
- Create: `android/app/src/main/res/mipmap-xhdpi/ic_launcher.png` (96x96)
- Create: `android/app/src/main/res/mipmap-xxhdpi/ic_launcher.png` (144x144)
- Create: `android/app/src/main/res/mipmap-xxxhdpi/ic_launcher.png` (192x192)
- Create: Foreground icons `ic_launcher_foreground.png` for all densities (108x108 up to 432x432)
- Create: `android/app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml`
- Create: `android/app/src/main/res/mipmap-anydpi-v26/ic_launcher_round.xml`

- [x] **Step 1:** Write a PowerShell script `android/scripts/generate-icons.ps1` utilizing `System.Drawing.Bitmap` to resize `Logo.png` into standard mipmap sizes.
- [x] **Step 2:** Run `android/scripts/generate-icons.ps1` to generate all density PNGs.
- [x] **Step 3:** Create adaptive XML drawables with dark background (`#121212`) and foreground logo.
- [x] **Step 4:** Verify icon files exist in all mipmap folders with correct dimensions.
- [x] **Step 5:** Commit: `git commit -m "feat(android): generate adaptive and legacy icons from Logo.png"`

---

### Task 7: Rust Engine JNI Integration (`engine/src/jni.rs`)

**Files:**
- Modify: `engine/Cargo.toml`
- Create: `engine/src/jni.rs`
- Modify: `engine/src/lib.rs` (or `engine/src/main.rs`)
- Create: `android/app/src/main/java/com/arzeck/aftertone/engine/AfterToneNative.kt`
- Test: `engine/tests/jni_test.rs`

- [x] **Step 1:** Add `[lib] crate-type = ["rlib", "cdylib"]` and `jni = "0.21"` dependency to `engine/Cargo.toml`.
- [x] **Step 2:** Write `engine/src/jni.rs` implementing:
  - `Java_com_arzeck_aftertone_engine_AfterToneNative_startServer(port: jint) -> jint`
  - `Java_com_arzeck_aftertone_engine_AfterToneNative_decryptSaavn(env, url: jstring) -> jstring`
  - `Java_com_arzeck_aftertone_engine_AfterToneNative_parseTtml(env, ttml: jstring) -> jstring`
  - `Java_com_arzeck_aftertone_engine_AfterToneNative_isServerRunning() -> jboolean`
  - `Java_com_arzeck_aftertone_engine_AfterToneNative_stopServer()`
- [x] **Step 3:** Implement Kotlin bridge `AfterToneNative.kt` in `android/app/src/main/java/com/arzeck/aftertone/engine/` loading library `libaftertone_engine`.
- [x] **Step 4:** Add unit tests in `engine/tests/` to verify Rust engine JNI functions and compilation.
- [x] **Step 5:** Run `cargo test --manifest-path engine/Cargo.toml --release` to ensure all 16 existing tests plus JNI tests pass.
- [x] **Step 6:** Commit: `git commit -m "feat(engine): add JNI bindings for Android client"`

---

### Task 8: Wire Listen Together Party Protocol to Engine Server

**Files:**
- Modify: `android/app/src/main/java/com/arzeck/aftertone/service/party/` (or repository)
- Create: `android/app/src/main/java/com/arzeck/aftertone/engine/PartyBridge.kt`

- [x] **Step 1:** Wire the Android party client to communicate with AfterTone's party server format (6-character uppercase codes, JSON action messages `Join`, `Leave`, `SyncState`, `Heartbeat`).
- [x] **Step 2:** Wire monotonic clock synchronization matching the engine's NTP-style ping/pong calculation.
- [x] **Step 3:** Provide an in-app toggle: "Local Wi-Fi Host" (which spawns engine party server via `AfterToneNative.startServer`) vs. "Remote Party Server".
- [x] **Step 4:** Commit: `git commit -m "feat(android): wire Listen Together to AfterTone party engine protocol"`

---

### Task 9: CI Workflow for Android Builds (`.github/workflows/android.yml`)

**Files:**
- Create: `.github/workflows/android.yml`

- [x] **Step 1:** Write `.github/workflows/android.yml` triggering on pushes to `main` and `feat/android`.
- [x] **Step 2:** Configure steps: Checkout repository, setup Java 21, setup Android SDK with NDK 26+ and CMake 3.22.1, setup Rust with `aarch64-linux-android` target.
- [x] **Step 3:** Run `./gradlew assembleDevDebug` from `android/`.
- [x] **Step 4:** Upload generated APK artifact (`android/app/build/outputs/apk/dev/debug/app-dev-debug.apk`).
- [x] **Step 5:** Commit: `git commit -m "ci: add GitHub Actions workflow for Android build"`

---

### Task 10: Legal, Attribution, Documentation & Verification

**Files:**
- Create: `NOTICE`
- Modify: `README.md`
- Modify: `android/README.md`

- [x] **Step 1:** Create root `NOTICE` detailing copyright and GPL-3.0 attribution for BitChord, Kushagra Singh, and all bundled libraries.
- [x] **Step 2:** Update root `README.md` adding Android build instructions using `gradlew.bat` and PowerShell.
- [x] **Step 3:** Run `git grep -i "bitchord"` across `android/` and verify that any remaining occurrences are strictly in copyright/upstream license notices.
- [x] **Step 4:** Run `git grep -i "api.bitchord"` to guarantee zero telemetry endpoints remain.
- [x] **Step 5:** Run `cargo test --manifest-path engine/Cargo.toml --release` to verify engine integrity.
- [x] **Step 6:** Commit: `git commit -m "docs: add Android build instructions, NOTICE attribution, and verification"`
