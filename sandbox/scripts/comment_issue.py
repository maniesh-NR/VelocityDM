#!/usr/bin/env python3
"""Comment on a GitHub issue (harness close-out helper)."""
import json
import sys
import urllib.request

TOKEN = open("/home/z/my-project/.gh_token").read().strip()
API = "https://api.github.com/repos/maniesh-NR/VelocityDM"

issue = sys.argv[1]
body = open(sys.argv[2]).read()

req = urllib.request.Request(
    f"{API}/issues/{issue}/comments",
    data=json.dumps({"body": body}).encode(),
    headers={
        "Authorization": f"Bearer {TOKEN}",
        "Accept": "application/vnd.github+json",
        "Content-Type": "application/json",
    },
    method="POST",
)
resp = json.load(urllib.request.urlopen(req))
print(f"comment {resp['id']} posted on issue #{issue} ({resp['html_url']})")
