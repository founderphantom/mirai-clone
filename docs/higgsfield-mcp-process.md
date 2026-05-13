# Higgsfield MCP Process

Validated on 2026-05-12 against `https://mcp.higgsfield.ai/mcp`.

This document records the working MCP flow for the two Higgsfield operations
Mirai needs:

1. Create/train a reusable Soul from 5-20 same-character images.
2. Generate an image with that Soul using one reference image and an empty app
   prompt.

## Validated Run

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

The response contains `user_id` when the access token is valid.

Use the access token for MCP:

```http
Authorization: Bearer <access-token>
Content-Type: application/json
Accept: application/json, text/event-stream
```

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

- `media_confirm` returned status `"uploaded"`.
- Passing confirmed `media_id` values into `show_characters` failed with:
  `"Expected a media_id UUID, completed image generation job ID, or https:// URL"`.
- Passing the returned public HTTPS `url` values into `show_characters`
  succeeded after the URLs were readable.

Use the public HTTPS `url` values for Soul training unless Higgsfield fixes the
media-ID path.

## Soul Creation

Working payload:

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

Wrangler env values should use MCP tool names:

```json
{
  "HIGGSFIELD_MCP_CLONE_TRAINING_TOOL": "show_characters",
  "HIGGSFIELD_MCP_GENERATION_TOOL": "generate_image"
}
```

Clone training should not pass app-shaped fields directly to MCP. It should:

1. Load the 5-20 selected clone reference images from R2.
2. Call `media_upload`.
3. `PUT` image bytes to the returned `upload_url` values.
4. Call `media_confirm`.
5. Wait until the returned HTTPS media URLs are fetchable.
6. Call `show_characters` with `action: "train"`, a clone name, and
   `images: [<https-url>, ...]`.
7. Poll `show_characters` with `action: "status"` until `ready`.
8. Store the returned `soul_id` as the clone provider Soul ID.

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
