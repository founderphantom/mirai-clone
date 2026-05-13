# Higgsfield Provider Process

Validated on 2026-05-12 against `https://mcp.higgsfield.ai/mcp`.
Auth endpoint behavior rechecked on 2026-05-13.
Agent REST upload and Soul creation behavior traced from `higgsfield 0.1.40`
on 2026-05-13.

This document records the working Higgsfield flows Mirai needs:

1. Create/train a reusable Soul from 5-20 same-character images.
2. Generate an image with that Soul using one reference image and an empty app
   prompt.

Clone training now uses the same Agent REST API as the Higgsfield CLI. Image
generation still uses MCP.

## Validated Product Clone Training Run

Successful local product-worker run on 2026-05-13:

- Training job ID: `train_51bc5c3c0e904c8cacd90145a4e24f4b`
- Clone ID: `clone_e2aa9cbe423d4b31b98ff7b99c48aa84`
- Display name: `My Soul`
- Provider account: `pa_higgsfield_founder`
- Provider Soul ID: `e6ef6721-cf69-4bc0-978d-a19f3045f6d9`
- Final status: `completed`
- Provider response preview:
  `{"id":"e6ef6721-cf69-4bc0-978d-a19f3045f6d9","name":"My Soul","status":"completed","type":"soul_2"}`

This Soul appeared in the Higgsfield dashboard. The successful path was the
Agent REST API flow documented below, not MCP `show_characters`.

## Validated MCP Generation Run

Training input:

- `.superpowers/browser-use-smoketest/maya-01.png`
- `.superpowers/browser-use-smoketest/maya-02.png`
- `.superpowers/browser-use-smoketest/maya-03.png`
- `.superpowers/browser-use-smoketest/maya-04.png`
- `.superpowers/browser-use-smoketest/maya-05.png`

Generation reference input:

- `.superpowers/browser-use-smoketest/starter-aiden.jpg`

Result:

- Soul ID: `1127b774-6cf9-4594-b8ea-1b46deeaba40`
- Generation job ID: `5dccb09d-4421-48b8-9adc-db7b610236f9`
- Provider model returned by MCP: `text2image_soul_v2`
- Output URL:
  `https://d8j0ntlcm91z4.cloudfront.net/user_37qkAfgwz3GdqEzgaYrhldNnp0T/hf_20260512_180559_5dccb09d-4421-48b8-9adc-db7b610236f9.png`

## Auth

Discovery:

```text
GET https://mcp.higgsfield.ai/.well-known/oauth-protected-resource
```

The metadata advertises the device-code flow at:

```text
https://fnf-device-auth.higgsfield.ai
```

Start device auth:

```json
POST https://fnf-device-auth.higgsfield.ai/authorize
{
  "clientName": "mirai-product-worker"
}
```

Poll token:

```json
POST https://fnf-device-auth.higgsfield.ai/token
{
  "device_code": "<device-code>"
}
```

Refresh access tokens:

```json
POST https://fnf-device-auth.higgsfield.ai/refresh
{
  "refresh_token": "<refresh-token>"
}
```

Current endpoint behavior: `refreshToken` returns a 422 validation error for a
missing `refresh_token` body field; `refresh_token` reaches token validation.
The refresh response includes a new `refresh_token`, so long-running workers
must account for token rotation rather than assuming the original refresh token
can be reused indefinitely.

Validate access tokens:

```json
POST https://fnf-device-auth.higgsfield.ai/validate
{
  "token": "<access-token>"
}
```

On 2026-05-13, `/validate` rejected refreshed device-flow access tokens with
`{"detail":"Invalid or missing API key"}` even though the same bearer token
successfully called MCP `tools/list`. Treat `/validate` as a warning-only
diagnostic endpoint, not as a required gate for clone training or generation.
The production auth flow should use `/refresh` plus direct MCP calls as the
source of truth.

Use the access token for MCP:

```http
Authorization: Bearer <access-token>
Content-Type: application/json
Accept: application/json, text/event-stream
```

## Agent REST Soul Creation

The Higgsfield CLI does not create Souls through MCP `media_upload` and
`show_characters`. It uses `https://fnf.higgsfield.ai` with the same bearer token
and these endpoints:

```http
POST /agents/uploads?type=image
PUT <upload_url>
POST /agents/uploads/<upload-id>/confirm?type=image
POST /agents/custom-references
GET /agents/custom-references/<soul-id>
```

Headers used by the CLI:

```http
Authorization: Bearer <access-token>
Content-Type: application/json
X-Hf-Mcp-Client-Name: codex
```

`POST /agents/uploads?type=image` has an empty body and returns an upload ID,
public URL, and `upload_url`. After the `PUT`, `POST
/agents/uploads/<upload-id>/confirm?type=image` confirms the media input.

Soul creation payload:

```json
{
  "input_images": [
    { "id": "<upload-id-1>", "type": "media_input" },
    { "id": "<upload-id-2>", "type": "media_input" }
  ],
  "name": "My Soul",
  "type": "soul_2"
}
```

The returned `id` is the provider Soul ID. Poll `GET
/agents/custom-references/<soul-id>` until `status` is `ready` or `completed`.

## MCP Tools

The live MCP tools are generic tool names, not direct model names:

```text
show_characters  # Soul Character list/train/status
generate_image   # image generation, including Soul 2 / text2image_soul_v2
media_upload     # presigned upload URL creation
media_confirm    # confirms uploaded media
job_status       # generation polling
```

`text2image_soul_v2` is not an MCP tool name. It is the provider model returned
after calling `generate_image` with `params.model = "soul_2"`.

## Uploading Media

Call `media_upload` first:

```json
{
  "method": "upload_url",
  "files": [
    { "filename": "maya-01.png", "content_type": "image/png" },
    { "filename": "maya-02.png", "content_type": "image/png" }
  ]
}
```

For each returned upload:

1. `PUT` the image bytes to `upload_url` with the returned `content_type`.
2. Save both `media_id` and the returned public `url`.
3. Call `media_confirm`.

```json
{
  "type": "image",
  "media_ids": ["<media-id-1>", "<media-id-2>"]
}
```

Observed behavior:

- As of 2026-05-13, `media_upload` can return plain MCP text content with
  generated `curl -X PUT` commands instead of structured JSON upload slots.
  Parse the media ID, signed upload URL, and `Content-Type` from those lines.
  Use the signed URL for the PUT, then use the confirmed media ID as the
  provider reference value after `media_confirm`. Do not derive a public URL by
  stripping the signed URL query string; that CloudFront/S3 URL may still return
  `403`.
- Structured JSON upload responses may include a public HTTPS `url`. When they
  do, wait for that URL to become fetchable and use it as the provider reference
  value.
- `media_confirm` returned status `"uploaded"`.
- `show_characters` validates image references as media ID UUIDs, completed
  image generation job IDs, or HTTPS URLs.

## Legacy MCP Soul Creation

This was the previously validated MCP payload. The product worker no longer uses
it for clone training because MCP text-mode uploads can yield media IDs that
`show_characters` rejects even after `media_confirm`.

Working historical payload:

```json
{
  "jsonrpc": "2.0",
  "id": "maya-train",
  "method": "tools/call",
  "params": {
    "name": "show_characters",
    "arguments": {
      "action": "train",
      "name": "Maya MCP Warm URL Test 2026-05-12 135545",
      "images": [
        "https://d2ol7oe51mr4n9.cloudfront.net/.../maya-01.png",
        "https://d2ol7oe51mr4n9.cloudfront.net/.../maya-02.png",
        "https://d2ol7oe51mr4n9.cloudfront.net/.../maya-03.png",
        "https://d2ol7oe51mr4n9.cloudfront.net/.../maya-04.png",
        "https://d2ol7oe51mr4n9.cloudfront.net/.../maya-05.png"
      ]
    }
  }
}
```

Poll until ready:

```json
{
  "jsonrpc": "2.0",
  "id": "soul-status",
  "method": "tools/call",
  "params": {
    "name": "show_characters",
    "arguments": {
      "action": "status",
      "soul_id": "1127b774-6cf9-4594-b8ea-1b46deeaba40"
    }
  }
}
```

The status sequence was `training` until it became `ready`.

## Soul Image Generation

Upload and confirm the single reference image with the same media flow. Use the
returned public HTTPS URL as the generation media value.

Working payload:

```json
{
  "jsonrpc": "2.0",
  "id": "maya-generate",
  "method": "tools/call",
  "params": {
    "name": "generate_image",
    "arguments": {
      "params": {
        "model": "soul_2",
        "prompt": "",
        "soul_id": "1127b774-6cf9-4594-b8ea-1b46deeaba40",
        "medias": [
          {
            "value": "https://d2ol7oe51mr4n9.cloudfront.net/.../starter-reference.png",
            "role": "image"
          }
        ],
        "count": 1
      }
    }
  }
}
```

The submission response returned:

```json
{
  "id": "5dccb09d-4421-48b8-9adc-db7b610236f9",
  "status": "pending",
  "model": "text2image_soul_v2",
  "params": {
    "prompt": "",
    "soul_id": "1127b774-6cf9-4594-b8ea-1b46deeaba40",
    "quality": "2k"
  }
}
```

Poll generation:

```json
{
  "jsonrpc": "2.0",
  "id": "generation-status",
  "method": "tools/call",
  "params": {
    "name": "job_status",
    "arguments": {
      "jobId": "5dccb09d-4421-48b8-9adc-db7b610236f9",
      "sync": true
    }
  }
}
```

## Prompt Behavior

The app-side generation payload used:

```json
"prompt": ""
```

No descriptive prompt was sent by the caller.

The completed `job_status` response later contained a long descriptive prompt
and:

```json
"enhance_prompt": true
```

That prompt was generated by Higgsfield from the reference image and model
defaults. It was not supplied by Mirai or the local MCP test script.

If Mirai requires strictly blank prompt semantics, test this generation payload
before implementation:

```json
{
  "params": {
    "model": "soul_2",
    "prompt": "",
    "enhance_prompt": false,
    "soul_id": "<soul-id>",
    "medias": [{ "value": "<https-reference-url>", "role": "image" }],
    "count": 1
  }
}
```

Until that is validated, the safe statement is:

- Mirai can send an empty prompt.
- Higgsfield may still infer/enhance a provider prompt from the reference image
  unless `enhance_prompt: false` is accepted and honored.

## Product App Implications

Wrangler env values should use the Agent API for clone training and MCP tool
names for generation:

```json
{
  "HIGGSFIELD_API_URL": "https://fnf.higgsfield.ai",
  "HIGGSFIELD_MCP_GENERATION_TOOL": "generate_image"
}
```

Production refresh-token setup should use the smoke check that exercises MCP
directly:

```sh
python3 scripts/higgsfield_device_auth.py --smoke --print-token
```

Store the latest printed refresh token from that run, because `/refresh`
rotates refresh tokens:

```sh
wrangler secret put HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER -c workers/product/wrangler.product.jsonc
```

Clone training should not pass app-shaped fields directly to MCP. It should:

1. Load the 5-20 selected clone reference images from R2.
2. For each image, call `POST /agents/uploads?type=image`.
3. `PUT` image bytes to the returned `upload_url`.
4. Call `POST /agents/uploads/<upload-id>/confirm?type=image`.
5. Call `POST /agents/custom-references` with `input_images` upload IDs.
6. Poll `GET /agents/custom-references/<soul-id>` until `ready`.
7. Store the returned `id` as the clone provider Soul ID.

Queue behavior for clone training:

1. `POST /api/clones/manual-upload` writes the clone/training rows and enqueues
   `SubmitCloneTraining`.
2. The submit queue message uploads all selected images and creates the Agent
   REST custom reference. This first queue execution is expected to be the long
   one because it performs every image upload.
3. After provider submission, the worker enqueues `PollCloneTraining` with a
   60 second delay.
4. Each poll calls `GET /agents/custom-references/<soul-id>`. If Higgsfield still
   reports `training`, the worker records the poll response and enqueues another
   delayed poll.
5. Wrangler logs one `QUEUE mirai-clone-training 1/1` line per consumed queue
   message. A normal successful Soul training run therefore shows one long submit
   line followed by multiple short poll lines until Higgsfield returns
   `completed` or `ready`.

Generation should:

1. Load the selected visual reference image from R2.
2. Upload it through `media_upload` and `media_confirm`.
3. Use the returned HTTPS URL in:

```json
{
  "params": {
    "model": "soul_2",
    "prompt": "",
    "soul_id": "<clone-provider-soul-id>",
    "medias": [{ "value": "<https-reference-url>", "role": "image" }],
    "count": 1
  }
}
```

4. Poll `job_status` until the generation is terminal.
