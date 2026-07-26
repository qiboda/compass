#!/bin/bash
# Upload parquet_data snapshot to Baidu Cloud via BaiduPCS-Go.
#
# Usage:
#   scripts/upload-parquet.sh                    # zip + upload to /compass/
#   scripts/upload-parquet.sh --keep-zip         # keep local zip after upload
#
# Requires: zip, baidupcs

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PARQUET_DIR="${PARQUET_DIR:-${PROJECT_ROOT}/parquet_data}"
TARGET_DIR="/compass"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
ZIP_NAME="parquet_data-${TIMESTAMP}.zip"
ZIP_PATH="/tmp/${ZIP_NAME}"
KEEP_ZIP=false

if [ "${1:-}" = "--keep-zip" ]; then
    KEEP_ZIP=true
fi

red()   { echo -e "\033[31m$*\033[0m" >&2; }
green() { echo -e "\033[32m$*\033[0m"; }
info()  { echo -e "\033[36m>>> $*\033[0m"; }

# --- preflight ---
if ! command -v baidupcs &>/dev/null; then
    red "error: baidupcs not found on PATH"
    exit 1
fi
if [ ! -d "$PARQUET_DIR" ]; then
    red "error: parquet_data/ not found at $PARQUET_DIR"
    exit 1
fi

# --- 1. Zip ---
info "Zipping $PARQUET_DIR -> $ZIP_PATH ..."
python3 -c "
import zipfile, os
with zipfile.ZipFile('$ZIP_PATH', 'w', zipfile.ZIP_DEFLATED) as zf:
    for root, dirs, files in os.walk('$PARQUET_DIR'):
        for f in files:
            if f.endswith('.DS_Store'): continue
            full = os.path.join(root, f)
            arcname = os.path.relpath(full, '$PARQUET_DIR')
            zf.write(full, arcname)
"
SIZE=$(du -h "$ZIP_PATH" | cut -f1)
green "  $ZIP_PATH ($SIZE)"

# --- 2. Upload ---
info "Uploading to Baidu Cloud $TARGET_DIR/$ZIP_NAME ..."
baidupcs upload "$ZIP_PATH" "$TARGET_DIR"
green "  Uploaded: $TARGET_DIR/$ZIP_NAME"

# --- 3. Cleanup ---
if ! $KEEP_ZIP; then
    rm -f "$ZIP_PATH"
    green "  Local zip removed"
fi

green "Done."
