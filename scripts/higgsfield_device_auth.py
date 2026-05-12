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


PROTECTED_RESOURCE_METADATA_URL = (
    "https://mcp.higgsfield.ai/.well-known/oauth-protected-resource"
)
DEVICE_AUTH_SERVER_FALLBACK = "https://fnf-device-auth.higgsfield.ai"
MCP_URL = "https://mcp.higgsfield.ai/mcp"
CLIENT_NAME = "mirai-product-worker"
SECRET_NAME = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER"
WRANGLER_CONFIG = "workers/product/wrangler.product.jsonc"
USER_AGENT = "OpenClaw/1.0 MCP Client"


class HttpJsonError(RuntimeError):
    def __init__(self, url: str, status: int, payload: dict[str, Any] | None, text: str):
        self.url = url
        self.status = status
        self.payload = payload or {}
        self.text = text
        super().__init__(f"{url} returned HTTP {status}: {text}")


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
        "--list-tools",
        action="store_true",
        help="list Higgsfield MCP tool names after auth completes",
    )
    parser.add_argument(
        "--client-name",
        default=CLIENT_NAME,
        help="client name sent to Higgsfield device auth",
    )
    parser.add_argument(
        "--mcp-url",
        default=MCP_URL,
        help="Higgsfield MCP JSON-RPC endpoint",
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


def get_json(url: str) -> dict[str, Any]:
    request = urllib.request.Request(
        url,
        headers={
            "accept": "application/json",
            "user-agent": USER_AGENT,
        },
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        try:
            payload = json.loads(detail)
        except json.JSONDecodeError:
            payload = None
        raise HttpJsonError(url, error.code, payload, detail) from error


def post_json(url: str, payload: dict[str, Any]) -> dict[str, Any]:
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "accept": "application/json",
            "content-type": "application/json",
            "user-agent": USER_AGENT,
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        try:
            payload = json.loads(detail)
        except json.JSONDecodeError:
            payload = None
        raise HttpJsonError(url, error.code, payload, detail) from error


def parse_json_or_sse(text: str) -> dict[str, Any]:
    try:
        payload = json.loads(text)
        if isinstance(payload, dict):
            return payload
    except json.JSONDecodeError:
        pass

    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("data:"):
            continue
        data = stripped.removeprefix("data:").strip()
        if not data or data == "[DONE]":
            continue
        try:
            payload = json.loads(data)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            return payload

    raise ValueError(f"response did not contain JSON: {text[:500]}")


def post_mcp_json(url: str, access_token: str, payload: dict[str, Any]) -> dict[str, Any]:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "accept": "application/json, text/event-stream",
            "authorization": f"Bearer {access_token}",
            "content-type": "application/json",
            "user-agent": USER_AGENT,
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return parse_json_or_sse(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        try:
            payload = parse_json_or_sse(detail)
        except ValueError:
            payload = None
        raise HttpJsonError(url, error.code, payload, detail) from error


def first_present(data: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        value = data.get(key)
        if value:
            return value
    return None


def discover_device_flow_urls() -> tuple[str, str, str]:
    auth_server = DEVICE_AUTH_SERVER_FALLBACK
    try:
        metadata = get_json(PROTECTED_RESOURCE_METADATA_URL)
    except Exception as error:
        print(
            f"warning: discovery failed; using {auth_server}: {error}",
            file=sys.stderr,
        )
    else:
        hints = metadata.get("higgsfield_auth_hints", {})
        options = hints.get("options") if isinstance(hints, dict) else None
        if isinstance(options, list):
            for option in options:
                if not isinstance(option, dict):
                    continue
                if option.get("flow") == "device_code" and option.get(
                    "authorization_server"
                ):
                    auth_server = str(option["authorization_server"])
                    break
        if auth_server == DEVICE_AUTH_SERVER_FALLBACK:
            servers = metadata.get("authorization_servers")
            if isinstance(servers, list):
                for server in servers:
                    if isinstance(server, str) and "fnf-device-auth" in server:
                        auth_server = server
                        break

    auth_server = auth_server.rstrip("/")
    return f"{auth_server}/authorize", f"{auth_server}/token", f"{auth_server}/refresh"


def authorize(authorize_url: str, client_name: str) -> dict[str, Any]:
    return post_json(authorize_url, {"clientName": client_name})


def poll_for_token(
    token_url: str,
    device_code: str,
    interval: float,
    timeout: float,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        time.sleep(interval)
        try:
            response = post_json(token_url, {"device_code": device_code})
        except HttpJsonError as error:
            response = error.payload
            if error.status >= 500 or not response:
                raise
        if first_present(response, "refreshToken", "refresh_token"):
            return response

        error = first_present(response, "error", "code", "detail")
        if error not in (None, "authorization_pending", "slow_down"):
            raise RuntimeError(f"token polling failed: {response}")
        if error == "slow_down":
            interval += 5

    raise TimeoutError("timed out waiting for Higgsfield device authorization")


def refresh_access_token(refresh_url: str, refresh_token: str) -> str:
    response = post_json(refresh_url, {"refreshToken": refresh_token})
    access_token = first_present(response, "accessToken", "access_token")
    if not access_token:
        raise RuntimeError(f"refresh response missing access token: {response}")
    return str(access_token)


def list_mcp_tools(mcp_url: str, access_token: str) -> list[dict[str, Any]]:
    response = post_mcp_json(
        mcp_url,
        access_token,
        {
            "jsonrpc": "2.0",
            "id": "tools-list",
            "method": "tools/list",
            "params": {},
        },
    )
    if response.get("error"):
        raise RuntimeError(f"MCP tools/list failed: {response['error']}")

    result = response.get("result")
    if not isinstance(result, dict):
        raise RuntimeError(f"MCP tools/list response missing result: {response}")

    tools = result.get("tools")
    if not isinstance(tools, list):
        raise RuntimeError(f"MCP tools/list response missing tools: {response}")

    return [tool for tool in tools if isinstance(tool, dict)]


def print_mcp_tools(tools: list[dict[str, Any]]) -> None:
    print("MCP tools:")
    for tool in tools:
        name = tool.get("name")
        if not name:
            continue
        description = str(tool.get("description") or "")
        description = " ".join(description.split())
        if description:
            print(f"- {name}: {description[:180]}")
        else:
            print(f"- {name}")


def main() -> int:
    args = parse_args()

    authorize_url, token_url, refresh_url = discover_device_flow_urls()
    auth = authorize(authorize_url, args.client_name)
    authorize_url = first_present(
        auth,
        "verificationUriComplete",
        "verification_uri_complete",
        "verificationUri",
        "verification_uri",
        "authorizeUrl",
        "authorize_url",
        "url",
    )
    device_code = first_present(auth, "deviceCode", "device_code")
    interval = args.poll_interval or float(first_present(auth, "interval") or 5)

    if not authorize_url or not device_code:
        raise RuntimeError(f"authorize response missing required fields: {auth}")

    print(f"Authorize here: {authorize_url}", flush=True)
    token = poll_for_token(token_url, str(device_code), interval, args.timeout)
    refresh_token = first_present(token, "refreshToken", "refresh_token")
    if not refresh_token:
        raise RuntimeError(f"token response missing refresh token: {token}")

    print("Refresh token received.")
    print(f"wrangler secret put {SECRET_NAME} -c {WRANGLER_CONFIG}")
    if args.list_tools:
        access_token = first_present(token, "accessToken", "access_token")
        if not access_token:
            access_token = refresh_access_token(refresh_url, str(refresh_token))
        tools = list_mcp_tools(args.mcp_url, str(access_token))
        print_mcp_tools(tools)
    if args.print_token:
        print(refresh_token)

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
