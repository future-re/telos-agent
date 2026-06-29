# Vendored Automation Runtime Inputs

Release builds should be prepared offline from files placed under this directory.

Preferred layout:

```text
desktop/src-tauri/vendor/
  python-runtime/
    telos-python-runtime-windows-x64.zip
```

The complete runtime archive should expand into `desktop/src-tauri/resources/` and contain:

```text
runtimes/python-windows-x64/python.exe
playwright-browsers/
runtime-manifest.json
```

Alternative assembly layout:

```text
desktop/src-tauri/vendor/
  python/
    python-windows-x64.zip
    get-pip.py
    wheelhouse-windows-x64/
  playwright/
    playwright-browsers-windows-x64.zip
```

`scripts/prepare-python-runtime.ps1` is offline by default. It only downloads when run with
`-AllowDownload`, which is intended for preparing and refreshing vendor artifacts, not release
builds.
