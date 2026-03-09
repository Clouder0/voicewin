#!/usr/bin/env bash
set -euo pipefail

if [[ "${VOICEWIN_LIVE_PROVIDER_TESTS:-0}" != "1" ]]; then
  echo "Skipping live provider smoke tests; set VOICEWIN_LIVE_PROVIDER_TESTS=1 to enable."
  exit 0
fi

if [[ -z "${ELEVENLABS_SCRIBE_V2_API_KEY:-}" ]]; then
  echo "ELEVENLABS_SCRIBE_V2_API_KEY is required when VOICEWIN_LIVE_PROVIDER_TESTS=1." >&2
  exit 1
fi

cargo test --locked -p voicewin-runtime --test live_provider_smoke -- --ignored --nocapture
