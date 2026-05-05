# Polar Sandbox Setup

Mirai uses Polar through the Better Auth Polar plugin. The app can run checkout
and portal flows before webhooks are configured. Webhooks are only needed for
server-side billing event snapshots and entitlement reconciliation.

## Local Sandbox Variables

Add these to `.dev.vars` in the project root:

```bash
POLAR_ACCESS_TOKEN="polar_oat_..."
POLAR_PRO_PRODUCT_ID="..."
POLAR_SERVER="sandbox"
POLAR_WEBHOOK_SECRET=""
```

Keep `POLAR_WEBHOOK_SECRET` blank until you have a public webhook URL.

## Product Setup

1. Open the Polar dashboard in sandbox mode.
2. Create a product for the paid plan, for example `Mirai Pro`.
3. Add the recurring price you want to test.
4. Copy the product ID from the product menu.
5. Paste it into `.dev.vars` as `POLAR_PRO_PRODUCT_ID`.

The app maps that product to the checkout slug `pro`, so the Account tab's
Upgrade button calls:

```ts
authClient.checkout({ slug: "pro" })
```

## What Works Without Webhooks

With only `POLAR_ACCESS_TOKEN` and `POLAR_PRO_PRODUCT_ID` configured:

- Better Auth can create Polar customers on signup.
- The Account tab can start sandbox checkout.
- The Account tab can open the Polar customer portal.

What will not be complete yet:

- `billing_events` will not receive order/subscription events.
- Mirai cannot reliably grant entitlements from Polar state yet.
- The app will not have a durable record that checkout succeeded.

## Webhook Setup Later

After the app has a public Cloudflare URL, create a Polar sandbox webhook endpoint:

```text
https://<your-worker-domain>/polar/webhooks
```

Then add the generated webhook secret:

```bash
POLAR_WEBHOOK_SECRET="..."
```

For Cloudflare production, set it with:

```bash
npx wrangler secret put POLAR_WEBHOOK_SECRET
```

## Temporary Local Webhook Option

If you want to test webhooks before deploy, expose local dev with a tunnel and
use the tunnel URL:

```bash
cloudflared tunnel --url http://localhost:5173
```

Then configure the Polar sandbox webhook as:

```text
https://<temporary-tunnel-host>/polar/webhooks
```

This tunnel URL changes unless you configure a named Cloudflare Tunnel, so it is
useful for testing but not production.
