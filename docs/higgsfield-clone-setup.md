# Higgsfield Clone Creation

Mirai clone profiles use Higgsfield Soul-ID as the current provider-side clone.
The local script uploads reference images, creates a Soul-ID, and returns the
`providerConfig` payload Mirai needs.

## Requirements

- Higgsfield CLI installed and logged in:

```bash
higgsfield auth login
```

- 5-20 reference images for the person or character clone.

## Create A Provider Clone

```bash
npm run higgsfield:clone:create -- \
  --name "Adam Phantom" \
  --image-dir ./references/adam \
  --wait \
  --output config/higgsfield/clones/adam-phantom.json
```

The result includes:

```json
{
  "providerConfig": {
    "customReferenceId": "<soul-id>",
    "styleStrength": 1
  }
}
```

Use that object as `providerConfig` when creating or updating a Mirai clone.

## Manual API Patch Example

After creating a clone in the UI, patch its provider config:

```bash
curl -X PATCH http://localhost:5173/api/clones/<clone-id> \
  -H "content-type: application/json" \
  -b "<browser session cookie>" \
  --data "{\"providerConfig\":{\"customReferenceId\":\"<soul-id>\",\"styleStrength\":1}}"
```

The next step is to expose this in the Mirai UI so a clone can display its
provider training status and saved Soul-ID without manual API work.
