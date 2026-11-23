#!/usr/bin/env python3
"""List repo issues (helper for harness close-outs)."""
import json
import sys
import urllib.request

TOKEN = open("/home/z/my-project/.gh_token").read().strip()
API = "https://api.github.com/repos/maniesh-NR/VelocityDM"


def req(path):
    r = urllib.request.Request(API + path, headers={
        "Authorization": f"Bearer {TOKEN}",
        "Accept": "application/vnd.github+json",
    })
    return json.load(urllib.request.urlopen(r))


if __name__ == "__main__":
    state = sys.argv[1] if len(sys.argv) > 1 else "all"
    for i in req(f"/issues?state={state}&per_page=50"):
        ms = i.get("milestone")
        print(f"#{i['number']:3} [{i['state']:6}] {i['title'][:78]} | ms={ms and ms['title']}")
