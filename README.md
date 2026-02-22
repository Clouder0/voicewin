# VoiceWin

## macOS unsigned build note

Current macOS artifacts are unsigned and not notarized.

If macOS shows `"VoiceWin is damaged and cannot be opened"`, move the app to
`/Applications` and run:

```bash
xattr -dr com.apple.quarantine "/Applications/VoiceWin.app"
```

Then open the app from Finder (right-click -> Open).
