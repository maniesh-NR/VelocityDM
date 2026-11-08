#!/usr/bin/env bash
# VelocityDM portable launcher (Linux/macOS).
# Keeps all state next to the binaries: ./velocitydm-data/
DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$DIR/velocitydm" --portable daemon
