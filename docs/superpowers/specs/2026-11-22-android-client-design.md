# Design: AfterTone Android Client (`android/`)

## 1. Intent & Context
AfterTone currently ships as a native Windows desktop player with a high-performance Rust core engine and a Compose Desktop UI. The goal of this task is to produce the standalone Android version of AfterTone located at `android/`, utilizing the upstream BitChord Android architecture (`app`, `shared`, `sharedUi`, `native`) as the base, integrating AfterTone's branding, adaptive icons, and Rust engine via JNI, while removing all BitChord-specific telemetry, donation links, and server dependencies.

---

## 2. Goals, Non-Goals, and Anti-Goals

### Goals
- Produce an Android client under `android/` that compiles and packages with `./gradlew assembleDebug` and `./gradlew assembleRelease`.
- Port BitChord's Android modules (`app`, `shared`, `sharedUi`, `native`) into `android/` with complete package renaming to `com.arzeck.aftertone`.
- Bridge AfterTone's Rust engine (`engine/`) using JNI to provide on-device 320kbps audio stream resolution, pure DES-ECB decryption, TTML syllable parsing, and local Wi-Fi Listen Together party server hosting.
- Replace BitChord branding, assets, and URLs:
  - Application ID: `com.arzeck.aftertone` (and `com.arzeck.aftertone.dev`)
  - App Name: `AfterTone`
  - Jam Invite Links: `https://aftertone.app/invite/<CODE>` and `aftertone://party/<CODE>`
  - Icons: Generated adaptive and legacy icons from root `Logo.png`
- Remove all external telemetry or requests to `api.bitchord.kushagrasingh.in`, and remove all donation/payment links (Ko-fi, PayPal, UPI).
- Retain full upstream GPL-3.0 attribution and copyright headers in ported files, updating root `README.md` and adding a `NOTICE` file.
- Provide a GitHub Actions workflow in `.github/workflows/android.yml` to build and upload the debug APK.

### Non-Goals
- Support for Android Wear OS (`wearos/`) or Android TV (`tv/`) in this initial release.
- Rewriting core Rust engine algorithms in Kotlin (DES-ECB, TTML parsing, and party server are bridged via JNI).
- Committing signing keystores or passwords (`keystore.properties.example` and `local.properties` convention only).

### Anti-Goals
- Sending any analytics or user data to BitChord's servers or third-party tracking services.
- Merging directly into `main` (all work remains strictly on `feat/android`).

---

## 3. Architecture & Module Structure

```text
android/
├── app/                  # Android application module (com.arzeck.aftertone)
│   ├── src/main/java/    # Android lifecycle, ExoPlayer PlaybackService, MediaSession, JNI bridge
│   ├── src/main/res/     # Branded themes, strings, layouts, mipmap icons from Logo.png
│   └── build.gradle.kts
├── shared/               # Kotlin Multiplatform domain/data module
│   ├── src/commonMain/   # Listen Together wire models, InnerTube client, lyrics interfaces
│   └── build.gradle.kts
├── sharedUi/             # Compose Multiplatform UI components
│   ├── src/commonMain/   # Now Playing screen, lyrics view, Material 3 frosted-glass (Haze) UI
│   └── build.gradle.kts
├── native/               # C++ CMake DSP routines for Automix
│   ├── analyzer/         # Mel spectrogram, tempo analysis, audio resampler
│   └── CMakeLists.txt
├── gradle/               # Gradle wrapper (Gradle 8.10+)
├── build.gradle.kts      # Root Android build script with AGP 8.10.1 & Kotlin 2.3.20
├── settings.gradle.kts   # Root Android settings including :app, :shared, :sharedUi
├── gradle.properties     # AndroidX and JVM options
├── keystore.properties.example # Template for release signing
└── gradlew / gradlew.bat # Gradle wrapper executables
```

---

## 4. Rust Engine JNI Integration

The Rust engine in `engine/` will be configured with `crate-type = ["rlib", "cdylib"]` and an Android JNI interface (`engine/src/jni.rs`):
1. `Java_com_arzeck_aftertone_engine_AfterToneNative_startServer(port: jint) -> jint`: Boots Tokio runtime and starts the local Axum HTTP/WebSocket server on `127.0.0.1:<port>`.
2. `Java_com_arzeck_aftertone_engine_AfterToneNative_decryptSaavn(env, url: jstring) -> jstring`: Direct zero-copy DES-ECB decryption.
3. `Java_com_arzeck_aftertone_engine_AfterToneNative_parseTtml(env, ttml: jstring) -> jstring`: Parses Apple Music TTML into structured JSON.
4. `Java_com_arzeck_aftertone_engine_AfterToneNative_isServerRunning() -> jboolean`: Health check.
5. `Java_com_arzeck_aftertone_engine_AfterToneNative_stopServer()`: Graceful shutdown.
