# Mirai App UI/UX Refresh — Design Spec

**Date:** 2026-05-06
**Approach:** A — Higgsfield-first inspiration, then code
**Scope:** All post-login screens + preset creator images
**Niches:** fashion / beauty / vibes / travel / content

---

## Overview

The logged-in app shell gets a design refresh informed by:
1. Generated UI inspiration images from Higgsfield GPT Image 2.0 (using landing page screenshots + screen context as prompts)
2. The landing page's design language as loose inspiration (not a 1:1 token match)

The Blitz screen is the primary/main screen. All other screens support it.

---

## Phase 1 — Higgsfield Image Generation

### 1A. Preset Creator Portraits

10 new portrait images replacing the scene images currently used in the starter grid (`starterImages` array in `OnboardingScreen.tsx`). Output path: `public/landing/starters/`.

Model: `gpt-image-2` via Higgsfield MCP.

Workflow:
1. Screenshot the live landing page with Chrome MCP for visual context
2. Generate each portrait with a prompt specifying ethnicity, niche, vibe, and creator-style framing
3. Review all 10 images before saving
4. Update `starterImages` array in `OnboardingScreen.tsx` to reference new paths

| Slug | Name | Ethnicity | Niche | Vibe |
|------|------|-----------|-------|------|
| sky-soft-glam | Sky | East Asian | Soft glam / beauty | Warm daylight, polished apartment |
| marina-coastal | Marina | Latina | Coastal / travel | Golden-hour ocean, linen |
| aiden-streetwear | Aiden | Black | Streetwear | City night, layered fits |
| noor-editorial | Noor | Middle Eastern | Editorial / fashion | Dramatic studio, bold makeup |
| juno-fitness | Juno | White | Fitness / wellness | Bright gym, activewear |
| valentin-luxury-travel | Valentin | South Asian | Luxury travel | Rooftop hotel, premium outfit |
| sienna-cottagecore | Sienna | White | Cottagecore | Garden, vintage dress |
| kai-cyber-night | Kai | East Asian | Cyber / nightlife | Neon streets, glossy jacket |
| maya-minimal-clean | Maya | Black | Minimal lifestyle | Neutral palette, city calm |
| rio-festival | Rio | Mixed / Afro-Latina | Festival / nightlife | Flash photography, expressive |

### 1B. UI Inspiration Images

5–8 screen-level inspiration images to guide CSS decisions. Each prompt feeds the landing page screenshot + screen-specific context. Output path: `public/landing/ui-inspiration/` (not shipped to users, used as design reference only).

| Screen | Prompt focus |
|--------|-------------|
| Blitz (home) | Full-bleed swipe deck, glassmorphism card label, fashion content |
| Onboarding / Starter picker | Portrait grid, creator card with niche badge, trendy |
| Library | Editorial masonry grid, image-first cards |
| Me page | Dark editorial panel, gradient header, premium stat chips |
| Create screen | Clean minimal form, section headers, fashion-forward |

---

## Phase 2 — App Shell CSS Refresh

Design direction is landing-inspired, not identical. The app feels like a native product; the landing page feels like marketing.

### Design Token Direction

| Property | Current app | Refresh direction |
|----------|-------------|-------------------|
| Accent | `#4fd1a5` (underused) | More prominent in active states, badges, chips |
| Card radius | 18px | 20px |
| Card shadow | `0 10px 24px rgba(15,15,15,0.05)` | Deeper: `0 16px 32px rgba(15,15,15,0.10)` |
| Heading weight | Mixed | Inter 700–800 for all screen titles |
| Background | `#f7f5ef` (warm beige) | Keep — distinctive brand feel |

### Per-Screen Direction

**Blitz (main screen)**
- Full-bleed swipe cards inheriting the `lp-deck__card` glass-label pattern
- Action buttons (like/skip) styled as pill chips with accent color
- No visual noise — image first, controls minimal

**Onboarding / Starter Grid**
- Portrait images at 3:4 aspect ratio (was 4:5)
- Name + niche tag overlaid at bottom of card (glassmorphism label, matching landing deck style)
- Adopt button lives inside the card on hover/focus, not below it

**Library**
- Two-column grid, editorial card feel
- Image dominates, metadata minimal below

**Me Page — flashiest screen**
- Dark gradient header panel (deep charcoal / `#101318` matching landing CTA banner)
- Stat chips (generations, saves, plan) displayed as accent-colored pills
- Profile name in Inter 800
- Plan badge prominent

**Create / Clones**
- Section headers styled like landing `lp-kicker` (small caps, accent color, weight 700)
- Form inputs consistent spacing, grouped visually
- Submit button full-width pill

**Bottom Nav**
- Active tab: accent-colored icon + small pill underline indicator
- Slightly taller (56px)
- Labels visible on active tab only

### Files to Edit

| File | Changes |
|------|---------|
| `src/client/app-ui.css` | Card radius/shadow, starter grid aspect ratio, source tabs, bubble chips |
| `src/client/styles.css` | Bottom nav, primary button, input fields |
| `src/client/mobile.css` | Bottom nav height, active state |
| `src/client/screens/OnboardingScreen.tsx` | `starterImages` array → new paths, card overlay pattern |
| `src/client/screens/MeScreen.tsx` | Dark header panel, stat chips |
| `src/client/screens/BlitzScreen.tsx` | Glass label on swipe cards, pill action buttons |
| `src/client/layout/BottomTabBar.tsx` | Active pill indicator |

---

## Phase 3 — Chrome MCP Review

1. Start dev server (`npm run dev`)
2. Open app in Chrome via MCP
3. Log in as `jam@gmail.com` / `jamaal123`
4. Walk each screen: Blitz → Onboarding → Library → Me → Create
5. Screenshot each screen, identify spacing/layout issues
6. Fix inline — no second review cycle unless major issues found

---

## Constraints

- Do not commit changes
- Do not modify server-side code
- Keep files under 500 lines
- Higgsfield images reviewed before being saved to `public/`
- GPT Image 2.0 (`gpt-image-2`) is the required model
