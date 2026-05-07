# Onboarding UI/UX Refresh Spec

Date: 2026-05-07
Status: Draft for approval
Execution gate: Do not start Ruflo implementation until the user approves this spec and the matching plan.

## Goal

Improve the logged-in `/onboarding` page for mobile and desktop. The refreshed screen should feel closer to the current `/me` page polish while staying consistent with the Mirai landing/app visual language. Higgsfield GPT Image 2 must be used to generate design inspiration and bubble artwork, with current screenshots supplied as image context before any source changes are made.

## Current Context

- App stack: React 19, TypeScript, Vite, CSS custom properties, Hono/Cloudflare Worker backend.
- Route handling: `/onboarding` is an app route in `src/client/router.tsx`.
- Current onboarding UI lives in `src/client/screens/OnboardingScreen.tsx`.
- Current onboarding CSS is split between `src/client/app-ui.css` and `src/client/mobile.css`.
- `/me` page structure and styling live in `src/client/screens/MeScreen.tsx` and `src/client/mobile.css`.
- Existing starter and UI inspiration assets already exist under `public/landing/starters/` and `public/landing/ui-inspiration/`.
- AgentDB CLI memory search returned no design-token entries in this workspace; project docs and screenshots are the reliable design-token sources.

## Requirements

1. Redesign `/onboarding` only, with supporting scoped CSS and tiny helper extraction if needed.
2. Preserve the existing onboarding flows:
   - Instagram harvest
   - Manual upload
   - Starter Soul adoption
   - Inspiration bubble selection
3. Preserve existing API contracts unless a change is explicitly approved later.
4. Do not alter auth, billing, generation, or server behavior.
5. Design must work at mobile and desktop sizes:
   - Mobile: 390x844 and 430x932
   - Tablet: 768x1024
   - Desktop: 1440x900 and 1920x1080
6. Use `/me` as UI inspiration:
   - Strong account-style hero surface
   - Clear status chips
   - Premium card hierarchy
   - Polished stat/action rhythm
7. Use landing/app tokens as inspiration, not as a strict lock:
   - `--green: #4fd1a5`
   - `--cta: #111318`
   - `--surface: #ffffff`
   - `--surface-2: #f8fafc`
   - Inter font, 700-900 heading weight
   - Rounded cards around 20px unless a circular bubble is intentional
8. Use Higgsfield CLI with model `gpt_image_2` for:
   - Mobile onboarding UI inspiration
   - Desktop onboarding UI inspiration
   - Bubble picker inspiration
   - Source-selection/Starter picker inspiration
   - Bubble background images focused on fashion, beauty, vibes, travel, and content niches
9. Pass screenshots of current design context into the image model:
   - Current `/onboarding` mobile and desktop
   - Current `/me` mobile and desktop
   - Current landing page mobile and desktop, if accessible
10. Review generated images with a model or browser agent capable of visual inspection before using them as implementation inspiration.
11. Inspiration bubbles must show aesthetic imagery inside circular/near-circular bubbles, with readable text centered over the image.
12. Bubble text must remain accessible, readable, and keyboard-focusable.
13. Do not store test credentials in repo files. Browser review uses environment variables populated by the operator.

## Non-Goals

- No new onboarding API endpoints.
- No D1 migration for bubble image URLs in this iteration.
- No changes to Soul Character creation logic.
- No changes to `/me` except using it as reference.
- No broad app refresh outside `/onboarding`.
- No replacement of starter creator portraits unless a generated review explicitly identifies a bad or inconsistent asset.

## Recommended Approach

Use a frontend-only refresh with generated static visual assets for bubble backgrounds. Add a small pure helper that maps dynamic bubble titles/slugs to approved visual assets. This avoids a backend migration while satisfying the visual bubble requirement.

Rejected alternatives:

- Add `image_url` to `inspiration_bubbles`: more durable long-term, but adds migration/API/test scope for a UI refresh.
- Refresh all logged-in screens: too broad and duplicates the 2026-05-06 app refresh plan.
- Use existing gallery assets only: fastest, but does not satisfy the explicit GPT Image 2 Higgsfield requirement.

## SPARC

### Specification

The user sees a polished onboarding hub after login. It should make the setup path feel clear and premium:

- A stronger hero/status area inspired by `/me`.
- Source selection that feels like a mode picker, not plain tabs.
- Starter cards remain image-forward.
- Bubble selection becomes a visual taste picker with image-filled bubbles and centered text.
- The UI must remain clear when no clone exists, when a clone is pending, during harvest polling, and after bubble selection.

Acceptance criteria:

- `/onboarding` renders without console errors.
- Existing API calls and tracking calls are preserved.
- Instagram, upload, starter, and bubble modes remain reachable.
- The bubble mode supports up to 5 selections and visibly marks selected bubbles.
- Bubble controls have `aria-pressed` and keyboard focus styling.
- Text does not overflow circular bubbles on mobile.
- Desktop layout uses horizontal space instead of appearing as a stretched mobile stack.
- New generated images are reviewed before being referenced by source.

### Pseudocode

```text
OnboardingScreen:
  load onboarding state
  derive clone, starters, bubbles, selected bubbles, mode availability
  render onboarding hero:
    show app kicker, headline, compact status chips, visual preview
  render source selector:
    for each source mode:
      show icon, label, short status
      disabled bubbles until clone exists
  if clone exists:
    render pending/ready status strip
  render active panel:
    instagram -> form + harvest progress
    upload -> upload form
    starter -> image card grid
    bubbles -> visual bubble grid + save CTA
  bubble card:
    visual = bubbleVisualFor(bubble)
    render image background
    render centered title and short label
    show selected check badge when active
```

```text
bubbleVisualFor(bubble):
  normalize slug/title/vibe summary
  check ordered niche rules:
    fashion/streetwear/editorial -> fashion image
    beauty/glam/skincare/grwm -> beauty image
    travel/coastal/airport/rooftop -> travel image
    content/camera/creator/social -> content image
    vibes/neon/festival/night/y2k -> vibes image
    wellness/pilates/fitness -> wellness image
  fallback to vibes image
```

### Architecture

New or changed frontend files:

- `src/client/screens/OnboardingScreen.tsx`
  - Keep state and API functions.
  - Restructure JSX for hero, source selector, and bubble visual cards.
  - Add `aria-pressed` to selectable source/bubble buttons where applicable.
  - Import `bubbleVisualFor`.
- `src/client/screens/onboarding-visuals.ts`
  - Pure mapping from bubble metadata to approved static image paths.
  - Exported for unit tests.
- `src/client/onboarding.css`
  - New scoped stylesheet for onboarding-specific layout and visual treatment.
  - Imported from `src/client/main.tsx` after existing app styles to override narrowly.
- `src/client/main.tsx`
  - Add one CSS import.
- `tests/client/onboarding-visuals.test.ts`
  - Validate common bubble categories and fallback behavior.

Generated assets:

- `public/landing/ui-inspiration/onboarding/*.jpg`
  - Design-reference images only, not imported by source.
- `public/landing/onboarding-bubbles/*.jpg`
  - Approved bubble background images used by `onboarding-visuals.ts`.

No backend changes are planned.

### Refinement

Risk controls:

- If `gpt_image_2` does not accept screenshot media inputs in the current Higgsfield CLI schema, stop and ask for approval before changing model or dropping image inputs.
- If generated bubble images include text or illegible composition, regenerate before source work.
- If a Ruflo reviewer cannot visually inspect image files, pause for Codex/GPT-5.5 or browser-agent image review before implementation.
- Keep CSS scoped in `onboarding.css` to avoid regressing `/me`, Blitz, Library, Create, or auth.
- Avoid adding dependencies unless a tester proves current tooling cannot verify the change.

### Completion

The work is complete only when:

- Approved Higgsfield images exist and are documented in the execution artifacts.
- `/onboarding` passes mobile and desktop browser review.
- `npm run typecheck`, `npm test`, and `npm run build` pass.
- Reviewer finds no blocking accessibility, visual, or regression issues.
- Final memory entry is stored with what worked and any follow-up risks.

## Open Assumptions

- The existing Higgsfield CLI auth is available or the operator can run `higgsfield auth login`.
- The requester-provided test login can be supplied through local environment variables for browser review.
- Public generated bubble assets are acceptable for onboarding UI decoration.
- A frontend-only bubble image mapping is sufficient for this iteration.

