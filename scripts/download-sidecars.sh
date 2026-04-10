#!/usr/bin/env bash
# Downloads FFmpeg, whisper.cpp CLI, and whisper model for Tauri sidecar bundling.
# Run from the agent/ directory: bash scripts/download-sidecars.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AGENT_DIR="$(dirname "$SCRIPT_DIR")"
BINARIES_DIR="$AGENT_DIR/src-tauri/binaries"
MODELS_DIR="$AGENT_DIR/src-tauri/resources/models"

FFMPEG_VERSION="n7.1-latest"
WHISPER_VERSION="v1.8.4"
WHISPER_MODEL="ggml-base.en.bin"

TRIPLE="x86_64-pc-windows-msvc"

mkdir -p "$BINARIES_DIR" "$MODELS_DIR"

# ── FFmpeg ─────────────────────────────────────────────────────────────────────
FFMPEG_TARGET="$BINARIES_DIR/ffmpeg-${TRIPLE}.exe"
if [ -f "$FFMPEG_TARGET" ] && [ "$(stat --printf='%s' "$FFMPEG_TARGET" 2>/dev/null || stat -f%z "$FFMPEG_TARGET" 2>/dev/null)" -gt 1000 ]; then
  echo "✓ FFmpeg already exists at $FFMPEG_TARGET"
else
  echo "→ Downloading FFmpeg ${FFMPEG_VERSION} (win64 GPL)..."
  FFMPEG_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-${FFMPEG_VERSION}-win64-gpl-7.1.zip"
  FFMPEG_ZIP="$BINARIES_DIR/ffmpeg.zip"

  curl -L --progress-bar -o "$FFMPEG_ZIP" "$FFMPEG_URL"
  echo "→ Extracting ffmpeg.exe..."

  unzip -o -j "$FFMPEG_ZIP" "*/bin/ffmpeg.exe" -d "$BINARIES_DIR"
  mv "$BINARIES_DIR/ffmpeg.exe" "$FFMPEG_TARGET"
  rm -f "$FFMPEG_ZIP"

  echo "✓ FFmpeg saved to $FFMPEG_TARGET"
fi

# ── whisper.cpp CLI ────────────────────────────────────────────────────────────
WHISPER_TARGET="$BINARIES_DIR/whisper-${TRIPLE}.exe"
if [ -f "$WHISPER_TARGET" ] && [ "$(stat --printf='%s' "$WHISPER_TARGET" 2>/dev/null || stat -f%z "$WHISPER_TARGET" 2>/dev/null)" -gt 1000 ]; then
  echo "✓ whisper.cpp CLI already exists at $WHISPER_TARGET"
else
  echo "→ Downloading whisper.cpp ${WHISPER_VERSION} (win-x64)..."
  WHISPER_URL="https://github.com/ggml-org/whisper.cpp/releases/download/${WHISPER_VERSION}/whisper-bin-x64.zip"
  WHISPER_ZIP="$BINARIES_DIR/whisper.zip"

  curl -L --progress-bar -o "$WHISPER_ZIP" "$WHISPER_URL"
  echo "→ Extracting whisper-cli.exe and required DLLs..."

  # Extract the CLI binary and its required DLLs.
  # whisper-cli.exe imports whisper.dll (which in turn imports the ggml*.dll
  # set). Missing whisper.dll causes whisper-cli.exe to fail with
  # STATUS_DLL_NOT_FOUND (-1073741515) at startup, which surfaces in the agent
  # as "Transcription failed (continuing anyway): whisper.cpp failed: ...".
  unzip -o -j "$WHISPER_ZIP" "Release/whisper-cli.exe" -d "$BINARIES_DIR"
  unzip -o -j "$WHISPER_ZIP" "Release/whisper.dll" "Release/ggml-base.dll" "Release/ggml-cpu.dll" "Release/ggml.dll" -d "$BINARIES_DIR"

  mv "$BINARIES_DIR/whisper-cli.exe" "$WHISPER_TARGET"
  rm -f "$WHISPER_ZIP"

  echo "✓ whisper.cpp CLI saved to $WHISPER_TARGET"
  echo "  (with whisper.dll, ggml-base.dll, ggml-cpu.dll, ggml.dll)"
fi

# ── Whisper model ──────────────────────────────────────────────────────────────
MODEL_TARGET="$MODELS_DIR/$WHISPER_MODEL"
if [ -f "$MODEL_TARGET" ] && [ "$(stat --printf='%s' "$MODEL_TARGET" 2>/dev/null || stat -f%z "$MODEL_TARGET" 2>/dev/null)" -gt 1000000 ]; then
  echo "✓ Whisper model already exists at $MODEL_TARGET"
else
  echo "→ Downloading whisper model ${WHISPER_MODEL} (~142 MB)..."
  MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/${WHISPER_MODEL}"

  curl -L --progress-bar -o "$MODEL_TARGET" "$MODEL_URL"
  echo "✓ Whisper model saved to $MODEL_TARGET"
fi

# ── Verify ─────────────────────────────────────────────────────────────────────
echo ""
echo "=== Verification ==="
for f in "$FFMPEG_TARGET" "$WHISPER_TARGET" "$BINARIES_DIR/whisper.dll" "$BINARIES_DIR/ggml-base.dll" "$BINARIES_DIR/ggml-cpu.dll" "$BINARIES_DIR/ggml.dll" "$MODEL_TARGET"; do
  if [ -f "$f" ]; then
    SIZE=$(stat --printf="%s" "$f" 2>/dev/null || stat -f%z "$f" 2>/dev/null || echo "?")
    echo "  ✓ $(basename "$f") — ${SIZE} bytes"
  else
    echo "  ✗ MISSING: $f"
    exit 1
  fi
done

echo ""
echo "All sidecars downloaded. Ready for 'npm run tauri build'."
