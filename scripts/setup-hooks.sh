#!/bin/bash
# Setup script to install pre-commit hooks for AnchorKit
# Run with: bash scripts/setup-hooks.sh

set -e

HOOKS_DIR=".git/hooks"
HOOK_FILE="pre-commit"

if [ ! -d "$HOOKS_DIR" ]; then
    echo "Error: .git/hooks directory not found. Are you in the project root?"
    exit 1
fi

echo "📦 Setting up pre-commit hooks..."

# Detect OS
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]; then
    # Windows
    echo "  → Installing Windows pre-commit hook..."
    cp scripts/pre-commit-hook.bat "$HOOKS_DIR/$HOOK_FILE"
    echo "  ✓ Pre-commit hook installed"
else
    # Unix-like (Linux, macOS)
    echo "  → Installing Unix pre-commit hook..."
    cp scripts/pre-commit-hook.sh "$HOOKS_DIR/$HOOK_FILE"
    chmod +x "$HOOKS_DIR/$HOOK_FILE"
    echo "  ✓ Pre-commit hook installed"
fi

echo ""
echo "✓ Setup complete!"
echo ""
echo "Pre-commit hook will now run automatically before each commit."
echo "To run checks manually, use: make check"
echo ""
