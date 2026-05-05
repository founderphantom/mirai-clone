# Mirai Secret Names

Set these with `wrangler secret put <NAME>`.

```bash
BETTER_AUTH_SECRET=
SCRAPECREATORS_API_KEY=
POLAR_ACCESS_TOKEN=
POLAR_WEBHOOK_SECRET=
POLAR_PRO_PRODUCT_ID=
HIGGSFIELD_JWT=
HIGGSFIELD_SESSION_ID=
HIGGSFIELD_CLIENT_COOKIE=
```

Use either `HIGGSFIELD_JWT` for short-lived local testing or `HIGGSFIELD_SESSION_ID` plus `HIGGSFIELD_CLIENT_COOKIE` for Worker-side token refresh.

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
