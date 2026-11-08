#!/usr/bin/env bash
# Portable mode launcher: keeps settings and downloads next to the binaries.
# Layout:  velocitydm-portable/
#            velocitydm          (GUI/CLI/daemon binary)
#            velocitydm-nmh      (browser native messaging host)
#            velocitydm.config.json
#            downloads/
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$HERE/downloads"
# the binary itself reads --portable; the flag is global so it works for all
# subcommands (daemon/gui/add/...)
exec "$HERE/velocitydm" --portable "$@"
