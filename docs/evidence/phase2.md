# Phase 2 local web UI verification

The Windows x64 release build serves the local conversion screen and uses the
Phase 1 runner for both audio presets.

The automated Playwright run used installed Microsoft Edge and the staged
Windows codec binaries. A test-only picker provided fixture paths for the
browser workflow. The browser test covered keyboard activation, radio selection,
light and dark themes, live status announcements, expanded diagnostics, corrupt
input recovery, both exports, reload during decode, cancellation and retry.
Independent ffprobe identified both completed exports. Desktop and narrow
viewport screenshots were inspected, and the narrow layout fit without
horizontal scrolling.

HTTP checks ran against the production executable and the browser test host.
They verified the launcher token, exact Host checking, same-origin mutation
requests, rejection of caller-supplied paths and protection of diagnostics.
The Windows release host also entered the modal wait for the real Open and Save
dialogs. Interaction with those dialogs remains unverified because Windows
computer-use initialization failed. The manual check was deferred before Phase 3.

Build and test with the Phase 0 bundle and a generated synthetic fixture. Use
Playwright 1.56.0 and installed Microsoft Edge. The test accepts a different
Chromium executable through `XFOLD_TEST_BROWSER`.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cargo-xfold.ps1 test --locked --offline --target x86_64-pc-windows-gnu
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cargo-xfold.ps1 build --release --locked --offline --target x86_64-pc-windows-gnu
$test = Get-ChildItem target/x86_64-pc-windows-gnu/debug/deps -Filter 'xfold-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
python scripts/test_web_ui.py $test.FullName
```

Set `XFOLD_TEST_NATIVE=1` to also check the real picker calls enter their modal
wait. It opens each dialog briefly and stops its isolated test host. If
`ui/app.ts` changes, regenerate `ui/app.js` with `node scripts/build_ui.cjs`
with TypeScript available, or pass the full path to `typescript.js`.
