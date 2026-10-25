# Installs rustup (if missing) and the toolchain pinned in rust-toolchain.toml.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Host 'rustup not found; installing...'
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements
    } else {
        $installer = Join-Path $env:TEMP 'rustup-init.exe'
        Invoke-WebRequest 'https://win.rustup.rs/x86_64' -OutFile $installer
        & $installer -y --default-toolchain none
        Remove-Item $installer
    }
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
}

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    throw 'rustup still not on PATH. Open a new terminal and re-run this script.'
}

# Reads rust-toolchain.toml and installs the channel and components it names.
rustup toolchain install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not (Get-Command link.exe -ErrorAction SilentlyContinue) -and
    -not (Test-Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe")) {
    Write-Warning 'MSVC build tools not detected. Install "Desktop development with C++" from https://visualstudio.microsoft.com/visual-cpp-build-tools/'
}

# uv provides pre-commit (and its own Python). See docs/adr/0002-git-hooks-for-local-checks.md.
if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    Write-Host 'uv not found; installing...'
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        winget install --id astral-sh.uv -e --accept-source-agreements --accept-package-agreements
    } else {
        Invoke-RestMethod 'https://astral.sh/uv/install.ps1' | Invoke-Expression
    }
    $env:Path = "$env:USERPROFILE\.local\bin;$env:LOCALAPPDATA\Microsoft\WinGet\Links;$env:Path"
}
if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    throw 'uv still not on PATH. Open a new terminal and re-run this script.'
}

uv tool install pre-commit
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$env:Path = "$env:USERPROFILE\.local\bin;$env:Path"

# Installs the hook types listed in default_install_hook_types.
pre-commit install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Reports anything still missing. See docs/adr/0003-cargo-xtask-for-project-commands.md.
cargo xtask doctor
exit $LASTEXITCODE
