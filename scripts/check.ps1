# Runs the same checks CI should: formatting, lints, tests, process-doc references.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

# Roadmap and plan docs are temporary; code must not reference them.
$refs = Get-ChildItem crates -Recurse -File | Where-Object { $_.FullName -notmatch '[\\/]target[\\/]' } |
    Select-String -Pattern 'docs[\\/](roadmap|plans)'
if ($refs) {
    $refs | ForEach-Object { Write-Host "$($_.Path):$($_.LineNumber): $($_.Line.Trim())" }
    Write-Error 'Code references docs/roadmap or docs/plans. See docs/development-process.md.'
}

cargo fmt --all --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
