#!/usr/bin/env python3
"""Acquire a Higgsfield device refresh token for the product worker secret."""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from typing import Any


AUTHORIZE_URL = "https://fnf-device-auth.higgsfield.ai/authorize"
TOKEN_URL = "https://fnf-device-auth.higgsfield.ai/token"
SECRET_NAME = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU"
WRANGLER_CONFIG = "workers/product/wrangler.product.jsonc"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Start Higgsfield device auth and print the Wrangler secret command.",
    )
    parser.add_argument(
        "--print-token",
        action="store_true",
        help="print the raw refresh token after auth completes",
    )
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=None,
        help="override token polling interval in seconds",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=600,
        help="maximum seconds to poll before exiting",
    )
    return parser.parse_args()


def post_json(url: str, payload: dict[str, Any]) -> dict[str, Any]:
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "accept": "application/json",
            "content-type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"{url} returned HTTP {error.code}: {detail}") from error


def first_present(data: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        value = data.get(key)
        if value:
            return value
    return None


def authorize() -> dict[str, Any]:
    return post_json(AUTHORIZE_URL, {"clientName": "mirai-product-worker"})


def poll_for_token(device_code: str, interval: float, timeout: float) -> dict[str, Any]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        time.sleep(interval)
        response = post_json(TOKEN_URL, {"deviceCode": device_code})
        if first_present(response, "refreshToken", "refresh_token"):
            return response

        error = first_present(response, "error", "code")
        if error not in (None, "authorization_pending", "slow_down"):
            raise RuntimeError(f"token polling failed: {response}")
        if error == "slow_down":
            interval += 5

    raise TimeoutError("timed out waiting for Higgsfield device authorization")


def main() -> int:
    args = parse_args()

    auth = authorize()
    authorize_url = first_present(
        auth,
        "verificationUriComplete",
        "verification_uri_complete",
        "authorizeUrl",
        "authorize_url",
        "url",
    )
    device_code = first_present(auth, "deviceCode", "device_code")
    interval = args.poll_interval or float(first_present(auth, "interval") or 5)

    if not authorize_url or not device_code:
        raise RuntimeError(f"authorize response missing required fields: {auth}")

    print(f"Authorize here: {authorize_url}", flush=True)
    token = poll_for_token(str(device_code), interval, args.timeout)
    refresh_token = first_present(token, "refreshToken", "refresh_token")
    if not refresh_token:
        raise RuntimeError(f"token response missing refresh token: {token}")

    print("Refresh token received.")
    print(f"wrangler secret put {SECRET_NAME} -c {WRANGLER_CONFIG}")
    if args.print_token:
        print(refresh_token)

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
