# Mirai Secret Names

Set these with `wrangler secret put <NAME>`.

```bash
BETTER_AUTH_SECRET=
SCRAPECREATORS_API_KEY=
POLAR_ACCESS_TOKEN=
POLAR_WEBHOOK_SECRET=
POLAR_PRO_PRODUCT_ID=
POLAR_STUDIO_PRODUCT_ID=
HIGGSFIELD_JWT=
HIGGSFIELD_SESSION_ID=
HIGGSFIELD_CLIENT_COOKIE=
HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU=
OPENROUTER_API_KEY=
OPENCODE_GO_API_KEY=
```

Use either `HIGGSFIELD_JWT` for short-lived local testing or `HIGGSFIELD_SESSION_ID` plus `HIGGSFIELD_CLIENT_COOKIE` for Worker-side token refresh.
Higgsfield tokens are stored as Cloudflare Secrets only. Raw Higgsfield tokens must not be written to D1.

For Polar sandbox setup, see `docs/polar-sandbox-setup.md`. Webhooks are optional until the app has a public URL; checkout can be tested with only `POLAR_ACCESS_TOKEN`, `POLAR_PRO_PRODUCT_ID`, and `POLAR_SERVER=sandbox`.

The session values are produced by the admin bootstrap script:

```bash
HIGGSFIELD_EMAIL=
HIGGSFIELD_PASSWORD=
HIGGSFIELD_AUTO_OTP=1
GAPI=
npm run higgsfield:session -- --write-env config/secrets/local.env
npm run higgsfield:session:publish
```

`HIGGSFIELD_EMAIL`, `HIGGSFIELD_PASSWORD`, `HIGGSFIELD_AUTO_OTP`, and `GAPI` are for the admin bootstrap process only. Do not set them as Worker runtime secrets.

For local setup, `GAPI` is optional. If `HIGGSFIELD_AUTO_OTP=1` is set but `GAPI` is missing, the script falls back to prompting for the email verification code.

## GitHub Actions Session Refresh

The scheduled workflow at `.github/workflows/higgsfield-session-refresh.yml` refreshes and publishes the shared Higgsfield session every 12 hours. Add these GitHub repository secrets:

```bash
HIGGSFIELD_EMAIL=
HIGGSFIELD_PASSWORD=
GAPI_COMMAND=
CLOUDFLARE_API_TOKEN=
CLOUDFLARE_ACCOUNT_ID=
```

`GAPI_COMMAND` must be a command available in the GitHub Actions runner that supports:

```bash
$GAPI_COMMAND gmail search "from:higgsfield newer_than:5m subject:verification" --max 1
$GAPI_COMMAND gmail get <message-id>
```

The workflow restores and saves `~/.higgsfield_session` with Actions cache, so email-code login only happens when Higgsfield/Clerk expires the cached session.
