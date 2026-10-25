# Runs every check in .pre-commit-config.yaml against the whole repo, as CI should.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

pre-commit run --all-files --hook-stage pre-commit
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pre-commit run --all-files --hook-stage pre-push
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
