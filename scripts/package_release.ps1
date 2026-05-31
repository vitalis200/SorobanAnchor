# package_release.ps1 — Build and bundle all SorobanAnchor release artifacts (Windows).
#
# Usage:
#   .\scripts\package_release.ps1 [-Version "0.2.0"]
#
# Outputs:
#   dist\anchorkit-<VERSION>.zip          — release zip archive
#   dist\anchorkit-<VERSION>\             — unpacked artifact directory
#
# Required tools: cargo, rustup

param(
    [string]$Version = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Resolve version ──────────────────────────────────────────────────────────
if (-not $Version) {
    $cargoToml = Get-Content "Cargo.toml" -Raw
    if ($cargoToml -match 'version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        Write-Error "Could not determine version from Cargo.toml"
        exit 1
    }
}

$DistDir    = "dist"
$BundleDir  = "$DistDir\anchorkit-$Version"
$ZipOut     = "$DistDir\anchorkit-$Version.zip"
$WasmTarget = "wasm32-unknown-unknown"
$WasmOut    = "target\$WasmTarget\release\anchorkit.wasm"

Write-Host "=== SorobanAnchor Release Packaging ===" -ForegroundColor Cyan
Write-Host "    Version : $Version"
Write-Host "    Bundle  : $BundleDir"
Write-Host "    Zip     : $ZipOut"
Write-Host ""

# ── Step 1: Ensure WASM target ───────────────────────────────────────────────
Write-Host "[1/6] Checking wasm32-unknown-unknown target..." -ForegroundColor Yellow
$installed = rustup target list --installed 2>&1
if ($installed -notmatch $WasmTarget) {
    Write-Host "      Installing $WasmTarget..."
    rustup target add $WasmTarget
}
Write-Host "      OK"

# ── Step 2: Build native CLI binary ─────────────────────────────────────────
Write-Host "[2/6] Building native CLI binary (release)..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "cargo build --release failed"; exit 1 }
Write-Host "      OK: target\release\anchorkit.exe"

# ── Step 3: Build WASM contract ──────────────────────────────────────────────
Write-Host "[3/6] Building WASM contract..." -ForegroundColor Yellow
cargo build --release --target $WasmTarget --no-default-features --features wasm
if ($LASTEXITCODE -ne 0) { Write-Error "WASM build failed"; exit 1 }
$wasmSize = (Get-Item $WasmOut).Length
Write-Host "      OK: $WasmOut ($([math]::Round($wasmSize/1KB, 1)) KB)"

# ── Step 4: Assemble bundle directory ────────────────────────────────────────
Write-Host "[4/6] Assembling bundle at $BundleDir..." -ForegroundColor Yellow
if (Test-Path $BundleDir) { Remove-Item -Recurse -Force $BundleDir }
New-Item -ItemType Directory -Path "$BundleDir\configs"  | Out-Null
New-Item -ItemType Directory -Path "$BundleDir\docs"     | Out-Null
New-Item -ItemType Directory -Path "$BundleDir\schemas"  | Out-Null

# CLI binary
Copy-Item "target\release\anchorkit.exe" "$BundleDir\anchorkit.exe"

# WASM contract
Copy-Item $WasmOut "$BundleDir\anchorkit.wasm"

# Schema
Copy-Item "config_schema.json" "$BundleDir\schemas\config_schema.json"

# Configs
Get-ChildItem "configs\*.json","configs\*.toml" -ErrorAction SilentlyContinue |
    Copy-Item -Destination "$BundleDir\configs\"

# Docs
Copy-Item "README.md" "$BundleDir\README.md"
Copy-Item "LICENSE"   "$BundleDir\LICENSE"
Get-ChildItem "docs\*" -ErrorAction SilentlyContinue |
    Copy-Item -Destination "$BundleDir\docs\"

# VERSION file
$Version | Set-Content "$BundleDir\VERSION"

Write-Host "      Bundle contents:"
Get-ChildItem $BundleDir -Recurse -File | ForEach-Object {
    Write-Host "        $($_.FullName.Replace((Resolve-Path $BundleDir).Path, ''))"
}

# ── Step 5: Create zip archive ───────────────────────────────────────────────
Write-Host "[5/6] Creating zip archive $ZipOut..." -ForegroundColor Yellow
if (-not (Test-Path $DistDir)) { New-Item -ItemType Directory -Path $DistDir | Out-Null }
if (Test-Path $ZipOut) { Remove-Item $ZipOut }
Compress-Archive -Path $BundleDir -DestinationPath $ZipOut
$zipSize = (Get-Item $ZipOut).Length
Write-Host "      Zip size: $([math]::Round($zipSize/1KB, 1)) KB"

# ── Step 6: Generate checksum ────────────────────────────────────────────────
Write-Host "[6/6] Generating SHA-256 checksum..." -ForegroundColor Yellow
$hash = (Get-FileHash $ZipOut -Algorithm SHA256).Hash
$checksumFile = "$DistDir\anchorkit-$Version.sha256"
"$hash  anchorkit-$Version.zip" | Set-Content $checksumFile
Write-Host "      $hash"

Write-Host ""
Write-Host "=== Release packaging complete ===" -ForegroundColor Green
Write-Host "    Zip      : $ZipOut"
Write-Host "    Checksum : $checksumFile"
