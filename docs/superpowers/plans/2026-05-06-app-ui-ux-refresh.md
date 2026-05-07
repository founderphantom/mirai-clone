# Mirai App UI/UX Refresh — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refresh all post-login app screens with a fashion/content creator aesthetic guided by Higgsfield GPT Image 2.0 inspiration, and replace the 10 preset creator images with distinct AI-generated portraits of different ethnicities and niches.

**Architecture:** Three phases — (1) generate images first via Higgsfield MCP before touching any code, (2) apply CSS and component changes guided by those images, (3) Chrome MCP browser review to catch regressions. No server-side changes. No commits.

**Tech Stack:** React 19, TypeScript, CSS custom properties, Higgsfield MCP (`gpt-image-2`), Chrome Claude MCP (`mcp__claude-in-chrome__*`), Inter font, Framer Motion

---

## File Map

| File | What changes |
|------|-------------|
| `public/landing/starters/` | New directory — 10 AI-generated portrait images |
| `public/landing/ui-inspiration/` | New directory — 5–8 UI screen inspiration images (reference only, not referenced in code) |
| `src/client/screens/OnboardingScreen.tsx` | `starterImages` array updated to new paths; starter card restructured for overlay |
| `src/client/screens/MeScreen.tsx` | Dark gradient header section, plan badge, actions card |
| `src/client/app-ui.css` | Card radius/shadow, starter card overlay styles, swipe action button polish |
| `src/client/mobile.css` | Bottom tab active indicator, me-screen header styles |

---

## Task 1: Screenshot the live landing page

**Files:**
- No file changes — screenshots are input context for Task 2

- [ ] **Step 1: Start the dev server**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npm run dev
```

Expected: Vite server starts, listening on `http://localhost:5173` (or similar port shown in output).

- [ ] **Step 2: Load Chrome MCP tools**

Use ToolSearch to load the Chrome tab tools:
```
ToolSearch: "select:mcp__claude-in-chrome__tabs_context_mcp,mcp__claude-in-chrome__navigate,mcp__claude-in-chrome__computer"
```

- [ ] **Step 3: Get current Chrome tabs context**

Call `mcp__claude-in-chrome__tabs_context_mcp` to see what's open.

- [ ] **Step 4: Create a new tab and navigate to the landing page**

Call `mcp__claude-in-chrome__tabs_create_mcp` to open a fresh tab, then navigate to `http://localhost:5173`.

- [ ] **Step 5: Screenshot the hero section**

Use `mcp__claude-in-chrome__computer` with `action: "screenshot"` to capture the hero. Save mental note: landing uses white bg, mint green `#4fd1a5` accent, dark `#111318` CTA, Inter 800 for headings, pill-shaped buttons, glassmorphism cards with subtle shadows.

- [ ] **Step 6: Scroll to the gallery / creator card section and screenshot**

Use `mcp__claude-in-chrome__computer` with scroll action then screenshot. Note the portrait card style: 4:5 aspect, rounded 12px, glassmorphism label at bottom.

- [ ] **Step 7: Screenshot the feature grid and pricing section**

One more screenshot capturing the card grid layout (16px gap, 20px radius, subtle border `#f0f0f0`, white bg cards).

---

## Task 2: Generate UI screen inspiration images

**Files:**
- Create: `public/landing/ui-inspiration/` (5 images — design reference only, not referenced in source code)

- [ ] **Step 1: Load the Higgsfield skill**

```
Skill: higgsfield-generate
```

Follow the skill's setup steps (workspace selection, balance check) before generating.

- [ ] **Step 2: Generate — Blitz screen inspiration**

Use `mcp__claude_ai_Higgsfield__generate_image` with model `gpt-image-2`:

```
Prompt: "Mobile app swipe card screen for a fashion AI content creator app, full-bleed portrait photo of stylish creator on card, glassmorphism label overlay at bottom with name and subtitle text, heart and X pill action buttons below card, dark gradient footer on card, mint green accent color #4fd1a5, white app shell, Inter bold typography, clean minimal iOS-style design, editorial fashion aesthetic, 9:16 vertical mobile screen mockup"
```

- [ ] **Step 3: Generate — Onboarding starter picker screen inspiration**

```
Prompt: "Mobile app screen showing a 2-column grid of AI creator profile cards, each card has a portrait photo with a dark glassmorphism overlay at the bottom showing creator name and niche tag in white text, mint green status badge, 10 diverse creator thumbnails including fashion, beauty, travel, fitness niches, white clean background, soft card shadows, Inter font, iOS mobile mockup 9:16"
```

- [ ] **Step 4: Generate — Library screen inspiration**

```
Prompt: "Mobile app media library screen showing a 2-column editorial photo grid, each cell has a fashion or beauty photo, minimal metadata label below, white background, 20px card radius, subtle drop shadows, mint green accent buttons, clean minimal iOS app design, Inter typography, vertical 9:16 mobile mockup"
```

- [ ] **Step 5: Generate — Me page inspiration (flashiest)**

```
Prompt: "Mobile app profile and settings screen, dark charcoal gradient header panel color #101318 to #1a2030 taking top 40% of screen, circular avatar with mint green glow border, bold white username in Inter 800, mint green accent stats chips showing generation count and plan tier badge, white card section below with action buttons, premium editorial fashion app aesthetic, vertical 9:16 iOS mockup"
```

- [ ] **Step 6: Generate — Create screen inspiration**

```
Prompt: "Mobile app content creation screen, clean white card sections, small mint green pill kicker label in caps above each section heading, pill-shaped dark CTA submit button at bottom, minimal form inputs with soft borders, Inter typography, fashion creator app aesthetic, vertical 9:16 iOS mockup"
```

- [ ] **Step 7: Review all 5 images**

Use `mcp__claude_ai_Higgsfield__job_display` on each job ID to see the results. Confirm each image is suitable as a design reference. If any image is off, regenerate with a tightened prompt.

- [ ] **Step 8: Download and save images**

Save each image to `public/landing/ui-inspiration/ui-blitz.jpg`, `ui-onboarding.jpg`, `ui-library.jpg`, `ui-me.jpg`, `ui-create.jpg`. These are reference only — not imported in any source file.

---

## Task 3: Generate preset creator portraits

**Files:**
- Create: `public/landing/starters/starter-sky.jpg`
- Create: `public/landing/starters/starter-marina.jpg`
- Create: `public/landing/starters/starter-aiden.jpg`
- Create: `public/landing/starters/starter-noor.jpg`
- Create: `public/landing/starters/starter-juno.jpg`
- Create: `public/landing/starters/starter-valentin.jpg`
- Create: `public/landing/starters/starter-sienna.jpg`
- Create: `public/landing/starters/starter-kai.jpg`
- Create: `public/landing/starters/starter-maya.jpg`
- Create: `public/landing/starters/starter-rio.jpg`

Generate all 10 with `mcp__claude_ai_Higgsfield__generate_image`, model `gpt-image-2`. Use portrait aspect ratio (3:4). After each batch, call `mcp__claude_ai_Higgsfield__job_status` to poll completion, then `mcp__claude_ai_Higgsfield__job_display` to review.

- [ ] **Step 1: Generate Sky — East Asian, soft glam beauty**

```
Prompt: "Professional portrait photo of a beautiful East Asian woman in her mid 20s, soft glam makeup with dewy skin, warm golden morning light through apartment window, wearing cream pastel cashmere set, styled hair, editorial lifestyle influencer aesthetic, clean minimalist background, fashion photography, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 2: Generate Marina — Latina, coastal travel**

```
Prompt: "Professional portrait photo of a stunning Latina woman in her mid 20s, natural minimal makeup, golden hour light at beach sunset, wearing linen white wide-leg pants and off-shoulder top, effortless coastal vacation vibes, relaxed luxury lifestyle influencer, editorial fashion photography, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 3: Generate Aiden — Black, streetwear**

```
Prompt: "Professional portrait photo of a stylish young Black man in his early 20s, confident expression, wearing layered streetwear outfit with oversized jacket and premium sneakers, city at night blurred background, neon accent lights, social media fashion influencer editorial look, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 4: Generate Noor — Middle Eastern, editorial fashion**

```
Prompt: "Professional portrait photo of a striking Middle Eastern woman in her late 20s, bold dramatic editorial makeup with glossy lip and sharp liner, wearing a dramatic sculptural fashion silhouette, dark clean studio backdrop, high fashion Vogue-style editorial photography, intense confident gaze, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 5: Generate Juno — White, fitness wellness**

```
Prompt: "Professional portrait photo of a fit white woman in her mid 20s, healthy natural glow no-makeup look, wearing chic matching activewear set in sage green, bright airy Pilates studio background, morning wellness routine lifestyle influencer, energetic but polished, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 6: Generate Valentin — South Asian, luxury travel**

```
Prompt: "Professional portrait photo of a handsome South Asian man in his late 20s, clean-cut confident look, wearing a premium linen shirt and tailored trousers, standing on a luxury hotel rooftop terrace with cityscape at dusk, elevated lifestyle and travel content creator aesthetic, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 7: Generate Sienna — White, cottagecore**

```
Prompt: "Professional portrait photo of a dreamy white woman in her early 20s, soft natural makeup with rosy cheeks, wearing a vintage floral midi dress, surrounded by lush garden flowers in soft diffused countryside daylight, cottagecore romantic lifestyle photography, gentle warm tones, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 8: Generate Kai — East Asian, cyber night**

```
Prompt: "Professional portrait photo of a cool East Asian person in their early 20s, androgynous futuristic streetwear with glossy jacket and bold accessories, neon city street at night with purple and blue reflections, cyber aesthetic nightlife fashion, sharp editorial photography, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 9: Generate Maya — Black, minimal lifestyle**

```
Prompt: "Professional portrait photo of an elegant Black woman in her late 20s, natural beauty minimal makeup, wearing a perfectly tailored neutral tonal outfit in beige and white, modern clean interior or contemporary gallery backdrop, calm city morning, minimal lifestyle creator aesthetic, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 10: Generate Rio — Mixed Afro-Latina, festival**

```
Prompt: "Professional portrait photo of a vibrant mixed Afro-Latina woman in her mid 20s, expressive joyful energy, wearing a bold colorful festival outfit with statement accessories, flash photography look simulating festival night, crowd blurred in background, playful fun social media content creator, 3:4 portrait ratio, photorealistic"
```

- [ ] **Step 11: Review all 10 portraits**

Use `mcp__claude_ai_Higgsfield__job_display` on each. Verify: distinct person visible, correct vibe, photorealistic, portrait orientation. Regenerate any that miss the mark.

- [ ] **Step 12: Save all 10 to `public/landing/starters/`**

Save as: `starter-sky.jpg`, `starter-marina.jpg`, `starter-aiden.jpg`, `starter-noor.jpg`, `starter-juno.jpg`, `starter-valentin.jpg`, `starter-sienna.jpg`, `starter-kai.jpg`, `starter-maya.jpg`, `starter-rio.jpg`.

---

## Task 4: Redesign starter cards in OnboardingScreen.tsx

**Files:**
- Modify: `src/client/screens/OnboardingScreen.tsx` lines 22–33 and 287–298

- [ ] **Step 1: Read the file**

```bash
cat -n "src/client/screens/OnboardingScreen.tsx"
```

- [ ] **Step 2: Update the `starterImages` array (line 22)**

Replace lines 22–33:
```tsx
const starterImages = [
  "/landing/clone-y2k-cafe.jpg",
  "/landing/clone-coastal-sunset.jpg",
  "/landing/clone-streetwear-berlin.jpg",
  "/landing/clone-dark-academia.jpg",
  "/landing/clone-cottagecore-picnic.jpg",
  "/landing/clone-tokyo-neon.jpg",
  "/landing/clone-cherry-blossom-seoul.jpg",
  "/landing/clone-retrofuturism.jpg",
  "/landing/clone-winter-minimalist.jpg",
  "/landing/clone-barbiecore.jpg"
];
```

With:
```tsx
const starterImages = [
  "/landing/starters/starter-sky.jpg",
  "/landing/starters/starter-marina.jpg",
  "/landing/starters/starter-aiden.jpg",
  "/landing/starters/starter-noor.jpg",
  "/landing/starters/starter-juno.jpg",
  "/landing/starters/starter-valentin.jpg",
  "/landing/starters/starter-sienna.jpg",
  "/landing/starters/starter-kai.jpg",
  "/landing/starters/starter-maya.jpg",
  "/landing/starters/starter-rio.jpg"
];
```

- [ ] **Step 3: Restructure the starter card JSX (lines 289–296)**

Replace:
```tsx
<button className="starter-card" key={starter.id} onClick={() => adoptStarter(starter)} disabled={busy}>
  <img src={starterImages[index % starterImages.length]} alt={starter.name} />
  <span>{starter.status === "setup_pending" ? "Setup pending" : "Ready"}</span>
  <strong>{starter.name}</strong>
  <p>{starter.persona}</p>
</button>
```

With:
```tsx
<button className="starter-card" key={starter.id} onClick={() => adoptStarter(starter)} disabled={busy}>
  <div className="starter-card-media">
    <img src={starterImages[index % starterImages.length]} alt={starter.name} />
    <div className="starter-card-overlay">
      <span>{starter.status === "setup_pending" ? "Setup pending" : "Ready"}</span>
      <strong>{starter.name}</strong>
      <p>{starter.persona}</p>
    </div>
  </div>
</button>
```

- [ ] **Step 4: Verify the file compiles**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npm run typecheck 2>&1 | tail -20
```

Expected: No TypeScript errors related to OnboardingScreen.

---

## Task 5: Update app-ui.css — card tokens + starter overlay styles

**Files:**
- Modify: `src/client/app-ui.css`

- [ ] **Step 1: Update card radius and shadow**

Replace lines 10–21 (the shared card block):
```css
.discovery-card,
.library-item,
.clone-list article,
.starter-card {
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 18px;
  background: var(--surface);
  color: var(--text);
  text-align: left;
  box-shadow: 0 10px 24px rgba(15, 15, 15, 0.05);
}
```

With:
```css
.discovery-card,
.library-item,
.clone-list article,
.starter-card {
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 20px;
  background: var(--surface);
  color: var(--text);
  text-align: left;
  box-shadow: 0 16px 32px rgba(15, 15, 15, 0.10);
}
```

- [ ] **Step 2: Update starter-card image aspect ratio**

Replace line 31:
```css
.discovery-card img,
.library-item img,
.starter-card img { aspect-ratio: 4 / 5; }
```

With:
```css
.discovery-card img,
.library-item img { aspect-ratio: 4 / 5; }
```

(Starter card images are now inside `.starter-card-media` and controlled separately.)

- [ ] **Step 3: Replace starter-card text styles with overlay styles**

Remove lines 36–61 (the block that styles `starter-card span`, `starter-card strong`, `starter-card p`):
```css
.discovery-card span,
.library-item div,
.clone-list article,
.starter-card strong,
.starter-card p,
.starter-card span {
  display: block;
  padding: 10px 12px;
}

.starter-card span {
  margin: 10px 10px 0;
  width: fit-content;
  border-radius: 999px;
  background: var(--green-soft);
  color: #059669;
  padding: 4px 9px;
  font-size: 11px;
  font-weight: 800;
}

.starter-card strong { padding-bottom: 3px; }

.starter-card p {
  padding-top: 0;
  color: var(--muted);
  font-size: 13px;
  line-height: 1.35;
}
```

And replace with:
```css
.discovery-card span,
.library-item div,
.clone-list article {
  display: block;
  padding: 10px 12px;
}

/* Starter card — portrait with glassmorphism overlay */
.starter-card-media {
  position: relative;
  aspect-ratio: 3 / 4;
}

.starter-card-media img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.starter-card-overlay {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  padding: 48px 14px 14px;
  background: linear-gradient(180deg, transparent, rgba(16, 19, 24, 0.82));
}

.starter-card-overlay span {
  display: inline-flex;
  margin-bottom: 6px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.18);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.28);
  color: #fff;
  padding: 3px 9px;
  font-size: 11px;
  font-weight: 700;
}

.starter-card-overlay strong {
  display: block;
  color: #fff;
  font-size: 15px;
  font-weight: 800;
  margin-bottom: 3px;
}

.starter-card-overlay p {
  margin: 0;
  color: rgba(255, 255, 255, 0.72);
  font-size: 12px;
  line-height: 1.35;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
```

- [ ] **Step 4: Verify no TypeScript/build errors**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npm run typecheck 2>&1 | tail -10
```

Expected: Clean.

---

## Task 6: Redesign MeScreen.tsx

**Files:**
- Modify: `src/client/screens/MeScreen.tsx`

- [ ] **Step 1: Replace the component JSX**

Replace the entire `return (...)` block (lines 21–57) with:
```tsx
  return (
    <div className="me-screen">
      <section className="me-header">
        <div className="me-avatar">
          <UserRound size={28} />
        </div>
        <h2>{account.user.name || account.user.email}</h2>
        <p className="me-server">Polar {account.billing.server}</p>
        <span className="me-plan-badge">Free</span>
      </section>

      <section className="me-actions">
        <button
          className="primary"
          disabled={!account.billing.checkoutEnabled}
          onClick={() => {
            track("checkout_started", { slug: "pro" });
            void authClient.checkout({ slug: "pro" });
          }}
        >
          <CreditCard size={16} />
          Upgrade to Pro
        </button>
        <button
          disabled={!account.billing.portalEnabled}
          onClick={() => {
            track("portal_opened");
            void authClient.customer.portal();
          }}
        >
          Billing Portal
        </button>
        <button className="me-signout" onClick={signOut} disabled={signingOut}>
          {signingOut ? <Loader2 className="spin" size={16} /> : <LogOut size={16} />}
          Sign out
        </button>
      </section>
    </div>
  );
```

- [ ] **Step 2: Verify typecheck**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npm run typecheck 2>&1 | tail -10
```

Expected: Clean.

---

## Task 7: Add me-screen styles to mobile.css + improve swipe actions + bottom tabs

**Files:**
- Modify: `src/client/mobile.css`

- [ ] **Step 1: Add me-screen styles**

Append the following block at the end of `mobile.css` (before the `@media (min-width: 1024px)` block):

```css
/* Me screen — dark editorial header */
.me-header {
  border-radius: 20px;
  overflow: hidden;
  background: linear-gradient(160deg, #101318 0%, #1a2030 100%);
  color: #fff;
  padding: 28px 20px 24px;
  display: grid;
  gap: 4px;
  justify-items: start;
  box-shadow: 0 16px 40px rgba(15, 15, 15, 0.18);
}

.me-avatar {
  width: 52px;
  height: 52px;
  border-radius: 999px;
  background: rgba(79, 209, 165, 0.18);
  border: 1.5px solid rgba(79, 209, 165, 0.4);
  color: #4fd1a5;
  display: grid;
  place-items: center;
  margin-bottom: 10px;
}

.me-header h2 {
  margin: 0;
  font-size: 22px;
  font-weight: 800;
  letter-spacing: -0.01em;
  color: #fff;
}

.me-server {
  margin: 0;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.5);
}

.me-plan-badge {
  margin-top: 10px;
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 4px 12px;
  font-size: 12px;
  font-weight: 800;
  background: rgba(79, 209, 165, 0.2);
  color: #4fd1a5;
  border: 1px solid rgba(79, 209, 165, 0.35);
}

.me-actions {
  display: grid;
  gap: 10px;
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 20px;
  background: var(--surface);
  box-shadow: 0 12px 32px rgba(15, 15, 15, 0.06);
}

.me-signout {
  min-height: 44px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: 1px solid rgba(220, 38, 38, 0.2);
  border-radius: 999px;
  background: rgba(254, 242, 242, 0.6);
  color: #dc2626;
  font-weight: 700;
  cursor: pointer;
  padding: 0 18px;
}

.me-signout:hover { background: #fef2f2; }
```

- [ ] **Step 2: Improve swipe action buttons**

Find and replace the existing `.swipe-actions .pass` and `.swipe-actions .like` lines (around line 382):

```css
.swipe-actions .pass { color: var(--coral); }
.swipe-actions .like { color: #059669; }
```

With:

```css
.swipe-actions .pass {
  color: var(--coral);
  border-color: rgba(255, 122, 89, 0.28);
}

.swipe-actions .like {
  color: #059669;
  border-color: rgba(79, 209, 165, 0.4);
  background: var(--green-soft);
}
```

- [ ] **Step 3: Polish bottom tabs active indicator**

Find the existing `.bottom-tabs .active` block (around line 179):

```css
.bottom-tabs .active {
  color: var(--text);
  background: var(--green-soft);
}
```

Replace with:

```css
.bottom-tabs .active {
  color: #047857;
  background: var(--green-soft);
  border-radius: 14px;
  font-weight: 700;
}

.bottom-tabs .active span {
  color: #047857;
}
```

- [ ] **Step 4: Typecheck**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npm run typecheck 2>&1 | tail -10
```

Expected: Clean.

---

## Task 8: Chrome MCP review — log in and walk every screen

**Files:**
- No new files — fix any issues found inline

- [ ] **Step 1: Load Chrome MCP tools**

```
ToolSearch: "select:mcp__claude-in-chrome__tabs_context_mcp,mcp__claude-in-chrome__navigate,mcp__claude-in-chrome__computer,mcp__claude-in-chrome__find,mcp__claude-in-chrome__javascript_tool"
```

- [ ] **Step 2: Navigate to the app and verify it loads**

Navigate to `http://localhost:5173`. Screenshot to confirm landing page renders.

- [ ] **Step 3: Log in**

Navigate to the auth/login flow. Enter credentials:
- Email: `jam@gmail.com`
- Password: `jamaal123`

Screenshot after login to confirm the Blitz screen loads.

- [ ] **Step 4: Review the Blitz screen**

Screenshot the Blitz screen. Check:
- Daily strip has green background and accent
- Swipe card fills the deck area
- Pass button is coral-tinted, Like button is green-tinted
- No layout overflow or broken elements

Fix any issues in `mobile.css` or `app-ui.css` before proceeding.

- [ ] **Step 5: Navigate to Onboarding (Starters tab)**

Navigate to the onboarding flow, click the Starters tab. Screenshot the starter grid. Check:
- New portrait images load (or placeholder gradient if images not yet present)
- Glassmorphism name overlay visible at bottom of each card
- 2-column grid with 20px radius cards
- Niche tag badge shows in glass chip style

Fix any broken styles before proceeding.

- [ ] **Step 6: Review the Me screen**

Navigate to the Me tab. Screenshot. Check:
- Dark gradient header (`#101318` → `#1a2030`) visible
- Avatar circle with green glow border
- Name displayed bold white
- Green plan badge chip
- Action buttons in white card below
- Sign out button in red-tinted style

Fix any issues before proceeding.

- [ ] **Step 7: Review the Library screen**

Navigate to Library. Screenshot. Check:
- 2-column grid renders
- Card radius 20px, shadow is slightly deeper
- No overflowing content

- [ ] **Step 8: Review the Create screen**

Navigate to Create. Screenshot. Check:
- Form layout clean
- Section headers styled
- No regressions

- [ ] **Step 9: Check desktop layout at 1024px+**

Use `mcp__claude-in-chrome__javascript_tool` to resize viewport:
```javascript
window.resizeTo(1280, 900);
```

Screenshot. Check sidebar renders, main content area has proper padding, starter grid stacks correctly on desktop.

- [ ] **Step 10: Fix any remaining issues found during review**

Apply targeted edits to `mobile.css` or `app-ui.css` for any spacing, overflow, or color issues discovered. Re-screenshot the affected screen to confirm.

---

## Constraints Reminder

- Do NOT commit any changes
- Do NOT modify any server-side files
- Do NOT create documentation files
- Images reviewed via `mcp__claude_ai_Higgsfield__job_display` before saving
- Model for all Higgsfield generation: `gpt-image-2`
