#!/usr/bin/env bash
# ==============================================================================
# TruLang VS Code Extension Publishing Script
#
# Supports:
# 1. Visual Studio Marketplace:
#    - Preferred: Microsoft Entra ID (--azure-credential)
#    - Transitional Fallback: VSCE_PAT (Retiring December 1, 2026)
# 2. Open VSX Registry:
#    - OVSX_PAT
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "============================================================"
echo " TruLang Official VS Code Extension Publisher"
echo "============================================================"

# 1. Validate clean build and package
echo ""
echo "[1/4] Building extension bundle..."
npm run build

echo ""
echo "[2/4] Packaging VSIX..."
npx @vscode/vsce package --no-dependencies

VSIX_FILE=$(ls -t trulang-*.vsix | head -n 1)
echo "Ready to publish: $VSIX_FILE"

# 2. Visual Studio Marketplace Publishing
echo ""
echo "[3/4] Publishing to Visual Studio Marketplace..."

HAS_AZURE_CLI=false
if command -v az >/dev/null 2>&1; then
    if az account show >/dev/null 2>&1; then
        HAS_AZURE_CLI=true
    fi
fi

if [ -n "${AZURE_CLIENT_ID:-}" ] || [ "$HAS_AZURE_CLI" = true ]; then
    echo ">> Authenticating via Microsoft Entra ID (--azure-credential)..."
    npx @vscode/vsce publish --azure-credential
    echo ">> Successfully published to Visual Studio Marketplace via Entra ID!"
elif [ -n "${VSCE_PAT:-}" ]; then
    echo ">> [DEPRECATION NOTICE] Using VSCE_PAT personal access token."
    echo ">> Note: Azure DevOps PATs are retiring on December 1, 2026."
    echo ">> Please migrate to Microsoft Entra ID (--azure-credential)."
    echo ">> Reference: https://code.visualstudio.com/api/working-with-extensions/publishing-extension#secure-automated-publishing-to-visual-studio-marketplace"
    npx @vscode/vsce publish -p "$VSCE_PAT"
    echo ">> Successfully published to Visual Studio Marketplace via VSCE_PAT!"
else
    echo ">> [SKIP] Visual Studio Marketplace authentication not configured."
    echo "   To publish, either:"
    echo "   a) Log in via Azure CLI ('az login') or set AZURE_CLIENT_ID / AZURE_TENANT_ID (Recommended)"
    echo "   b) Set VSCE_PAT environment variable (Retiring Dec 1, 2026)"
fi

# 3. Open VSX Publishing
echo ""
echo "[4/4] Publishing to Open VSX Registry..."

if [ -n "${OVSX_PAT:-}" ]; then
    echo ">> Authenticating with Open VSX Registry..."
    npx ovsx publish -p "$OVSX_PAT"
    echo ">> Successfully published to Open VSX Registry!"
else
    echo ">> [SKIP] OVSX_PAT not set. Skipping Open VSX Registry publish."
    echo "   To publish to Open VSX, set the OVSX_PAT environment variable."
fi

echo ""
echo "============================================================"
echo " Publishing pipeline finished."
echo "============================================================"
