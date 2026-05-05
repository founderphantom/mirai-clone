#!/usr/bin/env python3
"""Refresh the service Higgsfield session used by Mirai generation workers.

This is an admin bootstrap tool. It logs into the configured Higgsfield account
only when the cached Clerk session is missing or expired, then persists the
session id and __client cookie for Worker-side JWT refresh.
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

SESSION_CACHE = Path.home() / ".higgsfield_session"
SESSION_ENV_KEYS = {
    "HIGGSFIELD_SESSION_ID": "session_id",
    "HIGGSFIELD_CLIENT_COOKIE": "client_cookie",
}


def main() -> None:
    parser = argparse.ArgumentParser(description="Refresh Mirai's service Higgsfield session")
    parser.add_argument("--force-login", action="store_true", help="Ignore the cached session and request a new OTP")
    parser.add_argument("--write-env", type=Path, help="Update a dotenv-style file with Worker session variables")
    parser.add_argument("--publish-wrangler", action="store_true", help="Publish session values with wrangler secret put")
    parser.add_argument("--wrangler-env", help="Optional Wrangler environment name for secret publishing")
    parser.add_argument("--json", action="store_true", help="Print machine-readable status")
    parser.add_argument("--show-secrets", action="store_true", help="Include raw session values in stdout")
    args = parser.parse_args()

    try:
        session = refresh_session(force_login=args.force_login)
        if args.write_env:
            write_env_file(args.write_env, session)
        if args.publish_wrangler:
            publish_wrangler_secrets(session, args.wrangler_env)
    except Exception as exc:
        print(json.dumps({"status": "error", "message": str(exc)}))
        sys.exit(1)

    result = {
        "status": "success",
        "cache": str(SESSION_CACHE),
        "sessionId": maybe_secret(session["session_id"], args.show_secrets),
        "clientCookie": maybe_secret(session["client_cookie"], args.show_secrets),
        "wroteEnv": str(args.write_env) if args.write_env else None,
        "publishedWrangler": bool(args.publish_wrangler),
    }
    if args.json:
        print(json.dumps(result))
    else:
        print(f"Higgsfield session ready: {result['sessionId']}")
        print(f"Cache: {result['cache']}")
        if args.write_env:
            print(f"Updated env file: {args.write_env}")
        if args.publish_wrangler:
            print("Published Wrangler secrets.")


def refresh_session(force_login: bool = False) -> dict[str, str]:
    try:
        from dotenv import load_dotenv
        from higgsfield_api import _get_jwt_for_session, _load_session, login_full
    except ModuleNotFoundError as exc:
        raise RuntimeError(
            "Install Python Higgsfield dependencies first: "
            "python -m pip install curl_cffi python-dotenv Pillow"
        ) from exc

    load_dotenv()

    if force_login and SESSION_CACHE.exists():
        SESSION_CACHE.unlink()

    cached = _load_session()
    if cached:
        session_id, client_cookie = cached
        try:
            _get_jwt_for_session(session_id)
            return {"session_id": session_id, "client_cookie": client_cookie}
        except RuntimeError:
            SESSION_CACHE.unlink(missing_ok=True)

    email = os.environ.get("HIGGSFIELD_EMAIL", "")
    password = os.environ.get("HIGGSFIELD_PASSWORD", "")
    if not email or not password:
        raise RuntimeError("HIGGSFIELD_EMAIL and HIGGSFIELD_PASSWORD are required when the cache is expired.")

    login_full(email, password)
    cached = _load_session()
    if not cached:
        raise RuntimeError("Higgsfield login completed but did not write a session cache.")
    session_id, client_cookie = cached
    return {"session_id": session_id, "client_cookie": client_cookie}


def write_env_file(path: Path, session: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    existing = path.read_text().splitlines() if path.exists() else []
    remaining = set(SESSION_ENV_KEYS)
    lines: list[str] = []

    for line in existing:
        key = line.split("=", 1)[0].strip()
        if key in SESSION_ENV_KEYS:
            lines.append(format_env_line(key, session[SESSION_ENV_KEYS[key]]))
            remaining.discard(key)
        else:
            lines.append(line)

    for key in SESSION_ENV_KEYS:
        if key in remaining:
            lines.append(format_env_line(key, session[SESSION_ENV_KEYS[key]]))

    path.write_text("\n".join(lines) + "\n")


def publish_wrangler_secrets(session: dict[str, str], wrangler_env: str | None) -> None:
    npx = "npx.cmd" if os.name == "nt" else "npx"
    for key, session_key in SESSION_ENV_KEYS.items():
        command = [npx, "wrangler", "secret", "put", key]
        if wrangler_env:
            command.extend(["--env", wrangler_env])
        subprocess.run(command, input=f"{session[session_key]}\n", text=True, check=True)


def format_env_line(key: str, value: str) -> str:
    return f"{key}={json.dumps(value)}"


def maybe_secret(value: str, show: bool) -> str:
    if show:
        return value
    if len(value) <= 12:
        return "***"
    return f"{value[:6]}...{value[-4:]}"


if __name__ == "__main__":
    main()
