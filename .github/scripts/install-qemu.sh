#!/usr/bin/env bash
set -euo pipefail

# The Ubuntu runner's signed distribution repositories provide these tools.
# Scope both commands to that source list so unrelated vendor feeds cannot
# fail the update or change which packages are selected.
ubuntu_sources=/etc/apt/sources.list.d/ubuntu.sources
if [[ ! -r "$ubuntu_sources" ]]; then
    printf 'Missing Ubuntu APT source list: %s\n' "$ubuntu_sources" >&2
    exit 1
fi

options=(
    -o "Dir::Etc::sourcelist=$ubuntu_sources"
    -o 'Dir::Etc::sourceparts=-'
    -o 'Acquire::Retries=3'
)
sudo apt-get "${options[@]}" -o 'APT::Update::Error-Mode=any' update
sudo apt-get "${options[@]}" install -y --no-install-recommends qemu-system-x86 ovmf xorriso
