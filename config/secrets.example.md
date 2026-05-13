# Mirai Secret Names

Set runtime secrets with `wrangler secret put <NAME> -c workers/product/wrangler.product.jsonc`
or the matching auth Worker config.

```bash
BETTER_AUTH_SECRET=
POLAR_ACCESS_TOKEN=
POLAR_WEBHOOK_SECRET=
POLAR_PRO_PRODUCT_ID=
POLAR_STUDIO_PRODUCT_ID=
HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER=
OPENROUTER_API_KEY=
OPENCODE_GO_API_KEY=
```

## Higgsfield Provider Auth

The Rust product Worker uses a Higgsfield OAuth device refresh token seeded from
a Cloudflare Secret. Rotated refresh tokens live in Durable Object storage. Do
not write Higgsfield access or refresh tokens to D1.

Run:

```bash
python3 scripts/higgsfield_device_auth.py --smoke
```

The script prints an authorization URL and then, after approval, prints:

```bash
wrangler secret put HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER -c workers/product/wrangler.product.jsonc
```

Use `--print-token` only when you are ready to pipe the token directly into
`wrangler secret put`. Avoid saving raw token output in files, shell history, or logs.

## Polar

For Polar sandbox setup, see `docs/polar-sandbox-setup.md`. Webhooks are optional
until the app has a public URL. Checkout can be tested with `POLAR_ACCESS_TOKEN`
and at least one configured product ID.

## AI Providers

Workers AI uses the Cloudflare `AI` binding and does not need a separate secret.
OpenRouter and OpenCode Go keys are optional until routes call those providers.
