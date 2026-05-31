@echo off
REM Quality check script for AnchorKit (Windows)
REM Runs formatting and linting checks locally
REM Usage: quality-check.bat [target]
REM Targets: all (default), native, wasm

setlocal enabledelayedexpansion

set TARGET=%1
if "%TARGET%"=="" set TARGET=all

echo 🔍 Running quality checks for target: %TARGET%
echo.

if "%TARGET%"=="all" (
    echo 📋 Checking formatting...
    cargo fmt --all -- --check
    if errorlevel 1 exit /b 1
    echo ✓ Formatting OK
    echo.
    
    echo 🔗 Running clippy on all targets...
    cargo clippy --all-targets --all-features -- -D warnings
    if errorlevel 1 exit /b 1
    echo ✓ Clippy OK
    echo.
    
    echo 🧪 Running tests...
    cargo test
    if errorlevel 1 exit /b 1
    echo ✓ Tests OK
    
) else if "%TARGET%"=="native" (
    echo 📋 Checking formatting...
    cargo fmt --all -- --check
    if errorlevel 1 exit /b 1
    echo ✓ Formatting OK
    echo.
    
    echo 🔗 Running clippy on native targets...
    cargo clippy --lib --bins --tests --examples -- -D warnings
    if errorlevel 1 exit /b 1
    echo ✓ Clippy OK
    echo.
    
    echo 🧪 Running tests...
    cargo test
    if errorlevel 1 exit /b 1
    echo ✓ Tests OK
    
) else if "%TARGET%"=="wasm" (
    echo 📋 Checking formatting...
    cargo fmt --all -- --check
    if errorlevel 1 exit /b 1
    echo ✓ Formatting OK
    echo.
    
    echo 🔗 Running clippy on WASM target...
    cargo clippy --target wasm32-unknown-unknown --no-default-features --features wasm -- -D warnings
    if errorlevel 1 exit /b 1
    echo ✓ Clippy OK
    echo.
    
    echo 🏗️  Building WASM...
    cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm
    if errorlevel 1 exit /b 1
    echo ✓ WASM build OK
    
) else (
    echo Error: Unknown target '%TARGET%'
    echo Valid targets: all, native, wasm
    exit /b 1
)

echo.
echo ✓ All quality checks passed!
exit /b 0
