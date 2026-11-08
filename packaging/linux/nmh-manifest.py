#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Emits the NMH manifest JSON for the given host binary path."""
import json
import sys

host_path = sys.argv[1] if len(sys.argv) > 1 else "/usr/local/lib/velocitydm/velocitydm-nmh"
manifest = {
    "name": "com.velocitydm.nmh",
    "description": "VelocityDM native messaging host",
    "path": host_path,
    "type": "stdio",
    "allowed_extensions": ["velocitydm@velocitydm.local"],
}
print(json.dumps(manifest, indent=2))
