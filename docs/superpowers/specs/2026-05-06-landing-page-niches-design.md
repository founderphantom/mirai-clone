# Landing Page — Niche Research & Gallery Redesign

**Date:** 2026-05-06  
**Status:** Approved

---

## Overview

One-time enhancement to the Mirai landing page. A research swarm scrapes Instagram, TikTok, and Pinterest via ScrapeCreator CLI to build a comprehensive niche list (trending + high-engagement popular). Results are saved to agentDB. Higgsfield (GPT Image 2.0) generates new gallery and hero images per niche. The landing page is updated with static data embedded directly in source files.

---

## Phase 1 — Research Swarm

### Agents

Three parallel research agents coordinated by a lead:

| Agent | Platform | ScrapeCreator endpoints |
|-------|----------|------------------------|
| `ig-researcher` | Instagram | Reel search by keyword for each focus niche |
| `tiktok-researcher` | TikTok | Popular hashtags + keyword search for each focus niche |
| `pinterest-researcher` | Pinterest | Search for aesthetics and lifestyle keywords |

**Focus areas:** fashion, beauty, vibes, travel, content creation

### Coordinator responsibilities
- Waits for all 3 agent results via SendMessage
- Deduplicates niche labels across platforms
- Ranks by cross-platform engagement signal (views, pins, reel count)
- Preserves trending-first ordering
- Saves full ranked list to agentDB under namespace `landing/niches`
- Output shape per niche: `{ label: string, platforms: string[], engagementRank: number }`

### Swarm config
- Topology: hierarchical
- Max agents: 5 (coordinator + 3 researchers + 1 spare)
- Communication: SendMessage-first (no polling)

---

## Phase 2 — Higgsfield Image Generation

### Model
GPT Image 2.0 via Higgsfield CLI (not conservative — use full model capability)

### Gallery images (GalleryStrip)
- Portrait format, lifestyle photography aesthetic
- Mix of female and male characters — roughly 50/50
- One image per niche from the top niches in agentDB
- Saved to `public/landing/gallery/`
- Naming: `niche-<slug>.jpg`
- Images reviewed before embedding in page

### Hero deck images (SwipeDeckHero)
- Distinct from gallery images — different composition, closer crop, more editorial feel
- Same niche coverage but visually different shots
- Saved to `public/landing/hero/`
- Naming: `hero-<slug>.jpg`
- 6–8 images to match current deck count

### Review gate
Images are generated as files first. Review happens before source files are updated. No image goes live without manual approval.

---

## Phase 3 — Landing Page Updates

### SocialProofStrip
- Remove all hashtags from the `HASHTAGS` array
- Replace with niche labels from agentDB research (trending niches first)
- Target: 15–20 niche labels minimum
- Marquee becomes a true unlimited CSS loop:
  - Duplicate items 3× (not 2×) to prevent gap flicker at loop point
  - Single `animation: marquee-scroll linear infinite` with correct `-50%` (or `-33%`) translate endpoint
- No `#` prefix — plain niche labels (e.g., "Dark Academia", "Coastal Girl")

### GalleryStrip
- Replace ROW1/ROW2 static arrays with niche data from agentDB (embedded as static import)
- Trending niches appear first across both rows
- Split niches across two rows alternating (row1: odd-indexed, row2: even-indexed)
- Images: new Higgsfield-generated assets from `public/landing/gallery/`
- **Lazy loading**: Each `GalleryCard` uses `IntersectionObserver` — image `src` is set only when the card is within ~300px of the viewport. Gallery images use a `data-src` attribute until intersection fires.
- Unlimited loop: duplicate items 3× in each row track, CSS animation `translateX(-33.33%)` endpoint
- Hover-to-pause retained

### SwipeDeckHero
- Uses hero-specific images (`public/landing/hero/`) — not shared with gallery
- Improved swipe mechanics (researched via Context7):
  - Increase `dragConstraints` to allow fuller swipe range
  - Add `dragElastic` for natural rubber-banding
  - Use `useSpring` for smoother card settle animation
  - Add `dragMomentum: false` to prevent overshooting on fast flicks
  - Velocity-based dismiss threshold (not just offset): dismiss if `velocity.x > 400` regardless of offset
  - Background cards animate position with `layout` prop for fluid reordering
- Top card label shown below deck, transitions with `AnimatePresence`

### Lazy loading strategy (all images)
| Location | Strategy |
|----------|----------|
| Hero deck cards | `loading="eager"` — in viewport on load |
| Gallery cards | IntersectionObserver with 300px `rootMargin` — load just before scroll-in |
| SocialProofStrip | Text only, no images |

Implementation: a `useLazyImage(src, rootMargin)` hook returns the resolved `src` (undefined until observed). Cards render a placeholder div (same dimensions, background `#f0f0f0`) until resolved.

---

## Files Changed

| File | Change |
|------|--------|
| `src/client/screens/landing/GalleryStrip.tsx` | New images, lazy load hook, unlimited loop, niche data from static import |
| `src/client/screens/landing/SocialProofStrip.tsx` | Replace hashtags with niche labels, unlimited loop |
| `src/client/screens/landing/SwipeDeckHero.tsx` | New hero images, improved swipe physics |
| `src/client/screens/landing/landing.css` | Marquee and gallery track animation fixes |
| `src/data/landing-niches.ts` | Static export of ranked niche list from agentDB research |
| `public/landing/gallery/*.jpg` | New Higgsfield-generated gallery images |
| `public/landing/hero/*.jpg` | New Higgsfield-generated hero images |

---

## Out of Scope

- No Cloudflare Worker runtime API calls
- No recurring cron or scheduled refresh
- No changes to auth, pricing, or other landing sections
- No new npm dependencies beyond what's already installed (motion/react already present)

---

## Success Criteria

- [ ] SocialProofStrip shows 15+ niche labels, no hashtags, loops infinitely without gap
- [ ] GalleryStrip shows new Higgsfield images, trending niches first, lazy-loads off-screen cards
- [ ] Hero deck uses separate images from gallery, swipe feels fluid on mobile and desktop
- [ ] No layout shift on initial load (placeholders hold space)
- [ ] Chrome MCP review passes — no visual regressions on mobile and desktop viewports
