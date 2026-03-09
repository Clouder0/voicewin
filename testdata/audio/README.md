# Live Test Audio Fixtures

- `english.wav` is copied from `Uberi/speech_recognition` (`examples/english.wav`) under the project's BSD-3-Clause license.
- Source repository license: `https://github.com/Uberi/speech_recognition/blob/master/LICENSE.txt`
- Purpose: provide a tiny, deterministic speech sample for opt-in provider smoke tests.
- The live smoke test resamples the fixture to 16 kHz before sending it to ElevenLabs.
