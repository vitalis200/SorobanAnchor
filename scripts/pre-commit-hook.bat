@echo off
REM Pre-commit hook for AnchorKit (Windows)
REM Install with: copy scripts\pre-commit-hook.bat .git\hooks\pre-commit

setlocal enabledelayedexpansion

echo 🔍 Running pre-commit checks...

REM Check formatting
echo   - Checking code formatting...
cargo fmt --all -- --check >nul 2>&1
if errorlevel 1 (
    echo   ✗ Formatting issues found. Run 'cargo fmt --all' to fix.
    exit /b 1
)
echo   ✓ Formatting OK

REM Run clippy
echo   - Running clippy lints...
cargo clippy --all-targets --all-features -- -D warnings >nul 2>&1
if errorlevel 1 (
    echo   ✗ Clippy warnings found. Run 'cargo clippy --all-targets --all-features' to see details.
    exit /b 1
)
echo   ✓ Clippy OK

REM Run tests
echo   - Running tests...
cargo test --all-features >nul 2>&1
if errorlevel 1 (
    echo   ✗ Tests failed. Run 'cargo test' to see details.
    exit /b 1
)
echo   ✓ Tests OK

echo ✓ All pre-commit checks passed!
exit /b 0
