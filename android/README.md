# AfterTone Android Client

AfterTone Android is built on modern Kotlin Multiplatform and Jetpack Compose, featuring high-fidelity Material 3 design with dynamic artwork-driven themes, Apple Music-style animated karaoke lyrics, 320kbps pure DES stream decryption, and Listen Together real-time synchronized playback.

## Architecture

- `:app` — Android Application module (`com.arzeck.aftertone`). Houses ExoPlayer playback service, Android MediaSession integration, JNI native bridge, and download manager.
- `:shared` — Kotlin Multiplatform domain and data layer. Handles Listen Together models, InnerTube client, and lyrics sources.
- `:sharedUi` — Compose Multiplatform UI components, player screens, Haze frosted-glass effects, and dynamic palette extraction.
- `:native` — C++17 CMake routines for on-device Automix tempo estimation, mel-spectrogram analysis, and audio resampling.
- `libaftertone_native` — Bridged Rust core (`engine/`) providing DES-ECB decryption, TTML parsing, and local party server hosting.

## Building from Command Line

### Prerequisites
- JDK 21 (Eclipse Temurin or OpenJDK)
- Android SDK (API 36 / 37)
- Android NDK 26+ and CMake 3.22.1

### Windows (PowerShell)
```powershell
cd android

# Build Debug APK (Developer flavor)
.\gradlew.bat assembleDevDebug

# Build Release APK (Production flavor)
.\gradlew.bat assembleProdRelease
```

### Linux / macOS
```bash
cd android

# Build Debug APK
./gradlew assembleDevDebug

# Build Release APK
./gradlew assembleProdRelease
```

The compiled APKs will be located in:
- `android/app/build/outputs/apk/dev/debug/app-dev-debug.apk`
- `android/app/build/outputs/apk/prod/release/app-prod-release-unsigned.apk`
