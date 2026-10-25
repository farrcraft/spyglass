#!/bin/sh
# Runs every check in .pre-commit-config.yaml against the whole repo, as CI should.
set -eu
cd "$(dirname "$0")/.."

pre-commit run --all-files --hook-stage pre-commit
pre-commit run --all-files --hook-stage pre-push
