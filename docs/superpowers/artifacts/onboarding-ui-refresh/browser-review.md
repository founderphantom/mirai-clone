# Browser Review

Date: 2026-05-07

## Runtime

- Primary dev server path `/mnt/c/...` could not run Wrangler D1 locally because workerd SQLite shared-memory files failed on `/mnt/c`.
- Browser review used `/tmp/mirai-clone-run` with the same source synced from this repo, migrated local D1, and the test account already signed in.
- Review URL: `http://localhost:5173/onboarding`

## Screenshots

- Desktop source flow: `review-onboarding-desktop-cdp-source-row.png`
- Mobile source flow: `review-onboarding-mobile-viewport-source-row.png`
- Desktop bubble flow: `review-onboarding-bubbles-desktop.png`
- Mobile bubble flow: `review-onboarding-bubbles-mobile.png`

## Checks

- Desktop `/onboarding`: pass. Hero, image-backed source selector, pending clone banner, and bubble card layout fit without horizontal overflow.
- Mobile `/onboarding` at 390 x 844: pass. No document-level horizontal overflow. Source selector was changed to a horizontal card row so it does not run underneath the fixed bottom nav.
- Bubble UI: pass. Bubbles render as circles with generated aesthetic imagery and centered text. Labels and summaries remain readable under the dark overlay.
- Accessibility state: pass. Bubble buttons expose `aria-pressed`; selected bubbles switch from `Select ...` to `Remove ...` labels.

## Issues Found And Fixed

- The first mobile pass was too top-heavy; the hero pushed source cards into the fixed bottom nav. Fixed by tightening the mobile hero and changing mobile source cards to a horizontal row.
- The first bubble mapping routed "Tokyo Neon" and "Pilates Morning" to the content asset because rules used broad substring matching. Fixed with word-boundary matching, removed the broad `social` content term, and added focused tests.

## Validation

- `npx vitest run tests/client/onboarding-visuals.test.ts`: pass
- `npm run typecheck`: pass
- `npm test`: pass, 4 files / 16 tests
- `npm run build`: Vite mirai and client builds pass; Wrangler deploy dry-run exits with code 1 after selecting redirected config and reports a yargs validation error with no app-level error details.
