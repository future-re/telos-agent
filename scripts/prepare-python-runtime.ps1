param(
    [string]$Platform = "windows-x64",
    [string]$RuntimeBundleArchivePath = $env:TELOS_RUNTIME_BUNDLE_ARCHIVE_PATH,
    [string]$PythonZipPath = $env:TELOS_PYTHON_ZIP_PATH,
    [string]$GetPipPath = $env:TELOS_GET_PIP_PATH,
    [string]$WheelhousePath = $env:TELOS_PYTHON_WHEELHOUSE,
    [string]$PlaywrightBrowsersArchivePath = $env:TELOS_PLAYWRIGHT_BROWSERS_ARCHIVE,
    [string]$PythonZipUrl = $env:TELOS_PYTHON_ZIP_URL,
    [string]$PythonCommand = $env:TELOS_BUILD_PYTHON,
    [switch]$AllowDownload,
    [switch]$UseLocalPython,
    [switch]$SkipPlaywrightBrowserInstall
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$ResourceRoot = Join-Path $RepoRoot "desktop/src-tauri/resources"
$RuntimeRoot = Join-Path $ResourceRoot "runtimes"
$PythonHome = Join-Path $RuntimeRoot "python-$Platform"
$BrowserRoot = Join-Path $ResourceRoot "playwright-browsers"
$ManifestPath = Join-Path $ResourceRoot "runtime-manifest.json"
$TempRoot = Join-Path $RepoRoot ".telos/build/python-runtime"
$VendorRoot = Join-Path $RepoRoot "desktop/src-tauri/vendor"

if (-not $RuntimeBundleArchivePath) {
    $RuntimeBundleArchivePath = Join-Path $VendorRoot "python-runtime/telos-python-runtime-$Platform.zip"
}
if (-not $PythonZipPath) {
    $PythonZipPath = Join-Path $VendorRoot "python/python-$Platform.zip"
}
if (-not $GetPipPath) {
    $GetPipPath = Join-Path $VendorRoot "python/get-pip.py"
}
if (-not $WheelhousePath) {
    $WheelhousePath = Join-Path $VendorRoot "python/wheelhouse-$Platform"
}
if (-not (Test-Path $WheelhousePath)) {
    $fallbackWheelhouse = Join-Path $VendorRoot "python/wheelhouse"
    if (Test-Path $fallbackWheelhouse) {
        $WheelhousePath = $fallbackWheelhouse
    }
}
if (-not $PlaywrightBrowsersArchivePath) {
    $PlaywrightBrowsersArchivePath = Join-Path $VendorRoot "playwright/playwright-browsers-$Platform.zip"
}

$Packages = @(
    "beautifulsoup4",
    "html5lib",
    "httpx",
    "lxml",
    "markdownify",
    "openpyxl",
    "pandas",
    "pillow",
    "playwright",
    "pydantic",
    "pypdf",
    "python-docx",
    "python-pptx",
    "requests",
    "trafilatura",
    "xlsxwriter"
)

function New-CleanDirectory([string]$Path) {
    if (Test-Path $Path) {
        Remove-Item -Recurse -Force $Path
    }
    New-Item -ItemType Directory -Force $Path | Out-Null
}

function Get-PythonExe([string]$Home) {
    $exe = Join-Path $Home "python.exe"
    if (-not (Test-Path $exe)) {
        throw "Python executable not found at $exe"
    }
    return $exe
}

function Enable-EmbeddablePythonSite([string]$Home) {
    $pth = Get-ChildItem -Path $Home -Filter "python*._pth" | Select-Object -First 1
    if ($null -eq $pth) {
        return
    }
    $content = Get-Content $pth.FullName
    $content = $content | ForEach-Object {
        if ($_ -eq "#import site") { "import site" } else { $_ }
    }
    Set-Content -Path $pth.FullName -Value $content -Encoding ASCII
}

function Assert-OfflineFile([string]$Path, [string]$Name) {
    if (-not (Test-Path $Path)) {
        throw "$Name not found at $Path. Provide the file under desktop/src-tauri/vendor or rerun with -AllowDownload for non-release preparation."
    }
}

function Install-PythonPackages([string]$PythonExe) {
    if (Test-Path $WheelhousePath) {
        Write-Host "Installing bundled Python packages from offline wheelhouse: $WheelhousePath"
        & $PythonExe -m pip install --no-index --find-links $WheelhousePath --upgrade pip setuptools wheel
        & $PythonExe -m pip install --no-index --find-links $WheelhousePath $Packages
        return
    }

    if (-not $AllowDownload) {
        throw "Python wheelhouse not found at $WheelhousePath. Release runtime preparation is offline by default; provide a wheelhouse or pass -AllowDownload."
    }

    Write-Host "Installing bundled Python packages from PyPI because -AllowDownload was set"
    & $PythonExe -m pip install --upgrade pip setuptools wheel
    & $PythonExe -m pip install $Packages
}

function Write-RuntimeManifest([string]$PythonExe, [bool]$ChromiumBundled) {
    $Freeze = & $PythonExe -m pip freeze
    $PythonVersion = & $PythonExe -c "import sys; print(sys.version.split()[0])"
    $PlaywrightVersion = & $PythonExe -c "import importlib.metadata as m; print(m.version('playwright'))"

    $Manifest = [ordered]@{
        schema = 1
        status = "prepared"
        platform = $Platform
        python = @{
            executable = "runtimes/python-$Platform/python.exe"
            version = $PythonVersion
        }
        playwright = @{
            version = $PlaywrightVersion
            browsersPath = "playwright-browsers"
            chromiumBundled = $ChromiumBundled
        }
        packages = $Freeze
    }

    $Manifest | ConvertTo-Json -Depth 5 | Set-Content -Path $ManifestPath -Encoding UTF8
    Write-Host "Manifest written to $ManifestPath"
}

New-Item -ItemType Directory -Force $RuntimeRoot, $BrowserRoot, $TempRoot | Out-Null

if (Test-Path $RuntimeBundleArchivePath) {
    Write-Host "Using complete vendored runtime bundle: $RuntimeBundleArchivePath"
    New-CleanDirectory $RuntimeRoot
    New-CleanDirectory $BrowserRoot
    Expand-Archive -Path $RuntimeBundleArchivePath -DestinationPath $ResourceRoot -Force
    $PythonExe = Get-PythonExe $PythonHome
    if (-not (Test-Path $ManifestPath)) {
        Write-RuntimeManifest $PythonExe (-not $SkipPlaywrightBrowserInstall)
    }
    Write-Host "Python runtime unpacked from complete bundle at $PythonHome"
    exit 0
}

New-CleanDirectory $PythonHome

if (Test-Path $PythonZipPath) {
    Write-Host "Using vendored Python runtime zip: $PythonZipPath"
    Expand-Archive -Path $PythonZipPath -DestinationPath $PythonHome -Force
    Enable-EmbeddablePythonSite $PythonHome
    $PythonExe = Get-PythonExe $PythonHome

    if (Test-Path $GetPipPath) {
        Write-Host "Bootstrapping pip from vendored get-pip.py"
        & $PythonExe $GetPipPath
    } else {
        if (-not $AllowDownload) {
            Assert-OfflineFile $GetPipPath "get-pip.py"
        }
        $downloadedGetPip = Join-Path $TempRoot "get-pip.py"
        Write-Host "Downloading get-pip.py because -AllowDownload was set"
        Invoke-WebRequest -Uri "https://bootstrap.pypa.io/get-pip.py" -OutFile $downloadedGetPip
        & $PythonExe $downloadedGetPip
    }
} elseif ($UseLocalPython) {
    if (-not $PythonCommand) {
        $PythonCommand = "python"
    }
    Write-Host "Creating runtime from local Python command because -UseLocalPython was set: $PythonCommand"
    & $PythonCommand -m venv $PythonHome
    $PythonExe = Get-PythonExe $PythonHome
} elseif ($AllowDownload -and $PythonZipUrl) {
    $zipPath = Join-Path $TempRoot "python-$Platform.zip"
    Write-Host "Downloading Python runtime because -AllowDownload was set: $PythonZipUrl"
    Invoke-WebRequest -Uri $PythonZipUrl -OutFile $zipPath
    Expand-Archive -Path $zipPath -DestinationPath $PythonHome -Force
    Enable-EmbeddablePythonSite $PythonHome
    $PythonExe = Get-PythonExe $PythonHome

    $downloadedGetPip = Join-Path $TempRoot "get-pip.py"
    Write-Host "Downloading get-pip.py because -AllowDownload was set"
    Invoke-WebRequest -Uri "https://bootstrap.pypa.io/get-pip.py" -OutFile $downloadedGetPip
    & $PythonExe $downloadedGetPip
} else {
    throw "No offline Python runtime found. Put a complete bundle at $RuntimeBundleArchivePath or a Python zip at $PythonZipPath. Use -AllowDownload only for preparing vendor artifacts, not release builds."
}

Install-PythonPackages $PythonExe

$ChromiumBundled = $false
if (-not $SkipPlaywrightBrowserInstall) {
    if (Test-Path $PlaywrightBrowsersArchivePath) {
        Write-Host "Using vendored Playwright browsers archive: $PlaywrightBrowsersArchivePath"
        New-CleanDirectory $BrowserRoot
        Expand-Archive -Path $PlaywrightBrowsersArchivePath -DestinationPath $BrowserRoot -Force
        $ChromiumBundled = $true
    } elseif ($AllowDownload) {
        Write-Host "Installing Playwright Chromium because -AllowDownload was set"
        $env:PLAYWRIGHT_BROWSERS_PATH = $BrowserRoot
        & $PythonExe -m playwright install chromium
        $ChromiumBundled = $true
    } else {
        throw "Playwright browsers archive not found at $PlaywrightBrowsersArchivePath. Provide it for offline release builds or pass -SkipPlaywrightBrowserInstall."
    }
}

Write-RuntimeManifest $PythonExe $ChromiumBundled
Write-Host "Python runtime prepared at $PythonHome"
