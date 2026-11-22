# AfterTone Windows Installer Build Script
# Automates compiling the release binary and generating AfterTone-Setup.exe via Inno Setup

$ErrorActionPreference = "Stop"

$RootDir = (Resolve-Path "$PSScriptRoot\..\..").Path
Set-Location $RootDir

$llvmPath = "C:\Users\Aneesh\AppData\Local\LLVM\bin"
if (Test-Path $llvmPath) {
    $env:PATH = "$llvmPath;$env:PATH"
}

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "   AfterTone Windows Installer Generator" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# 1. Verify / Build Release Binary
$ReleaseExe = Join-Path $RootDir "engine\target\release\aftertone-native.exe"
if (-not (Test-Path $ReleaseExe)) {
    $RootTargetExe = Join-Path $RootDir "target\release\aftertone-native.exe"
    if (Test-Path $RootTargetExe) {
        $ReleaseExe = $RootTargetExe
    } else {
        Write-Host "`n[1/4] Building release binary with Cargo..." -ForegroundColor Yellow
        cargo build --release --manifest-path (Join-Path $RootDir "engine\Cargo.toml")
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo release build failed."
        }
    }
}
Write-Host "`n[1/4] Release binary found: $ReleaseExe" -ForegroundColor Green

# Ensure engine\target\release has the binary for Inno Setup if found in root target
$EngineTargetExe = Join-Path $RootDir "engine\target\release\aftertone-native.exe"
if (-not (Test-Path $EngineTargetExe) -and (Test-Path $ReleaseExe)) {
    $EngineTargetDir = Split-Path $EngineTargetExe -Parent
    if (-not (Test-Path $EngineTargetDir)) {
        New-Item -ItemType Directory -Path $EngineTargetDir -Force | Out-Null
    }
    Copy-Item $ReleaseExe $EngineTargetExe -Force
}

# 2. Verify Payload Assets
Write-Host "`n[2/4] Verifying required payload directories..." -ForegroundColor Yellow
$RequiredPaths = @(
    "windows\app",
    "windows\runtime",
    "web",
    "windows\WebView2Loader.dll",
    "windows\app\AfterTone.ico"
)
foreach ($p in $RequiredPaths) {
    $fullPath = Join-Path $RootDir $p
    if (-not (Test-Path $fullPath)) {
        throw "Missing required payload component: $p"
    }
}
Write-Host "All payload components present." -ForegroundColor Green

# 3. Locate or Install Inno Setup (ISCC)
Write-Host "`n[3/4] Locating Inno Setup Compiler (ISCC.exe)..." -ForegroundColor Yellow

function Find-ISCC {
    $cmd = Get-Command "ISCC" -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $candidates = @(
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )
    foreach ($cand in $candidates) {
        if (Test-Path $cand) { return $cand }
    }
    return $null
}

$IsccPath = Find-ISCC

if (-not $IsccPath) {
    Write-Host "Inno Setup compiler not found. Attempting install via winget..." -ForegroundColor Yellow
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        & winget install --id JRSoftware.InnoSetup -e --source winget --accept-source-agreements --accept-package-agreements --silent
        Start-Sleep -Seconds 3
        $IsccPath = Find-ISCC
    }
}

if (-not $IsccPath) {
    throw "Inno Setup (ISCC.exe) is required to build the installer. Please install Inno Setup 6 from https://jrsoftware.org/isdl.php"
}

Write-Host "Using Inno Setup Compiler: $IsccPath" -ForegroundColor Green

# 4. Compile Installer
Write-Host "`n[4/4] Compiling AfterTone-Setup.exe with LZMA2 compression..." -ForegroundColor Yellow
$DistDir = Join-Path $RootDir "windows\dist"
if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Path $DistDir -Force | Out-Null
}

$IssScript = Join-Path $RootDir "windows\installer\aftertone.iss"
& $IsccPath $IssScript

if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup compilation failed with exit code $LASTEXITCODE."
}

$OutputFile = Join-Path $DistDir "AfterTone-Setup-v1.0.0.exe"
if (-not (Test-Path $OutputFile)) {
    $OutputFile = Join-Path $RootDir "target\dist\AfterTone-Setup-v1.0.0.exe"
}
if (Test-Path $OutputFile) {
    $item = Get-Item $OutputFile
    $sizeMb = [math]::Round($item.Length / 1MB, 2)
    Write-Host "`n==========================================" -ForegroundColor Green
    Write-Host " ✅ INSTALLER BUILD SUCCEEDED!" -ForegroundColor Green
    Write-Host " File: $($item.FullName)" -ForegroundColor Cyan
    Write-Host " Size: $sizeMb MB" -ForegroundColor Cyan
    Write-Host "==========================================" -ForegroundColor Green
} else {
    throw "Expected output installer file not found at $OutputFile"
}
