#!/usr/bin/env python3

import sys

import requests

SERVER_URL = "http://localhost:3000"

request_type = None
if len(sys.argv) == 2:
    if sys.argv[1] in ("UnlockTimer", "LockTimer", "GetInfo"):
        request_type = sys.argv[1]

if request_type is None:
    sys.exit(f"Please specify a request: UnlockTimer, LockTimer, or GetInfo.")

print(requests.post(SERVER_URL, json={"type": request_type}).text)
