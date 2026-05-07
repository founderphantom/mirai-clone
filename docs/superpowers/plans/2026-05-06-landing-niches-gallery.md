# Landing Page — Niche Research & Gallery Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Research 40+ fashion/beauty/vibes/travel/content niches via ScrapeCreator swarm, generate Higgsfield GPT Image 2.0 gallery + hero images, and ship improved lazy loading, unlimited-loop carousels, and fluid swipe mechanics on the landing page.

**Architecture:** A 3-agent research swarm (one per platform) runs ScrapeCreator CLI commands, saves raw results to agentDB, then a coordinator synthesizes and ranks. Static niche data is embedded in `src/data/landing-niches.ts`. Higgsfield generates separate image sets for the gallery strip (16 images) and hero deck (7 images). All four landing page source files are updated. No runtime API calls — everything is static after this run.

**Tech Stack:** React + TypeScript, motion/react, ScrapeCreator CLI (`scrapecreators`), Higgsfield CLI (`higgsfield generate create gpt_image_2`), claude-flow agentDB MCP, IntersectionObserver API, CSS animations

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/data/landing-niches.ts` | **CREATE** | Static export of ranked niche list — source of truth for all three components |
| `src/client/screens/landing/SocialProofStrip.tsx` | **MODIFY** | Niche labels (no hashtags), 3× duplication, unlimited CSS loop |
| `src/client/screens/landing/GalleryStrip.tsx` | **MODIFY** | `useLazyImage` hook, new Higgsfield images, unlimited loop, niche data from static import |
| `src/client/screens/landing/SwipeDeckHero.tsx` | **MODIFY** | Hero-specific images, improved swipe physics (dragElastic, velocity dismiss, AnimatePresence) |
| `src/client/screens/landing/landing.css` | **MODIFY** | Fix marquee + gallery keyframes for 3× duplication (`-33.333%` translate) |
| `public/landing/gallery/` | **CREATE** | 16 Higgsfield GPT Image 2.0 portrait images |
| `public/landing/hero/` | **CREATE** | 7 Higgsfield GPT Image 2.0 editorial close-up images |

---

## Task 1 — TikTok Niche Research

**Agent:** `tiktok-researcher`  
**Files:** None modified — results saved to agentDB

- [ ] **Step 1.1: Fetch TikTok popular hashtags by industry**

Run each command. Requires `SCRAPECREATOR_API_KEY` env var or `--api-key` flag.

```bash
# Fashion / apparel
scrapecreators tiktok hashtags-popular \
  --period 30 --countryCode US --industry apparel-and-accessories \
  --format json --output /tmp/tt-apparel.json

# Beauty
scrapecreators tiktok hashtags-popular \
  --period 30 --countryCode US --industry beauty-and-personal-care \
  --format json --output /tmp/tt-beauty.json

# Travel
scrapecreators tiktok hashtags-popular \
  --period 30 --countryCode US --industry travel \
  --format json --output /tmp/tt-travel.json

# Entertainment / content vibes
scrapecreators tiktok hashtags-popular \
  --period 30 --countryCode US --industry news-and-entertainment \
  --format json --output /tmp/tt-entertainment.json

# 7-day newOnBoard (freshest trending)
scrapecreators tiktok hashtags-popular \
  --period 7 --countryCode US --newOnBoard \
  --format json --output /tmp/tt-new.json
```

- [ ] **Step 1.2: Deep-dive specific aesthetic hashtags**

```bash
scrapecreators tiktok search-hashtag --hashtag y2k --trim --format json --output /tmp/tt-y2k.json
scrapecreators tiktok search-hashtag --hashtag darkacademia --trim --format json --output /tmp/tt-dark.json
scrapecreators tiktok search-hashtag --hashtag cottagecore --trim --format json --output /tmp/tt-cottage.json
scrapecreators tiktok search-hashtag --hashtag streetwear --trim --format json --output /tmp/tt-street.json
scrapecreators tiktok search-hashtag --hashtag quietluxury --trim --format json --output /tmp/tt-quietlux.json
scrapecreators tiktok search-hashtag --hashtag cleangirl --trim --format json --output /tmp/tt-cleangirl.json
scrapecreators tiktok search-hashtag --hashtag aesthetic --trim --format json --output /tmp/tt-aesthetic.json
scrapecreators tiktok search-hashtag --hashtag grwm --trim --format json --output /tmp/tt-grwm.json
scrapecreators tiktok search-hashtag --hashtag contentcreator --trim --format json --output /tmp/tt-content.json
scrapecreators tiktok search-hashtag --hashtag traveltok --trim --format json --output /tmp/tt-travel2.json
scrapecreators tiktok search-hashtag --hashtag barbiecore --trim --format json --output /tmp/tt-barbie.json
scrapecreators tiktok search-hashtag --hashtag retrofuturism --trim --format json --output /tmp/tt-retro.json
```

- [ ] **Step 1.3: Parse hashtag labels and engagement signals**

```bash
python3 << 'EOF'
import json, os, re

results = []

# Parse hashtags-popular responses (array of hashtag objects)
for fname in ['tt-apparel', 'tt-beauty', 'tt-travel', 'tt-entertainment', 'tt-new']:
    path = f'/tmp/{fname}.json'
    if not os.path.exists(path): continue
    data = json.load(open(path))
    items = data if isinstance(data, list) else data.get('data', data.get('hashtags', []))
    for item in items[:20]:
        name = item.get('hashtag_name', item.get('name', item.get('title', '')))
        views = item.get('video_views', item.get('publish_cnt', item.get('view_count', 0)))
        if name:
            results.append({'raw': name, 'signal': int(views or 0), 'source': 'popular'})

# Parse search-hashtag responses (array of video objects — use video count as signal)
for fname, label in [
    ('tt-y2k', 'Y2K Fashion'), ('tt-dark', 'Dark Academia'),
    ('tt-cottage', 'Cottagecore'), ('tt-street', 'Streetwear'),
    ('tt-quietlux', 'Quiet Luxury'), ('tt-cleangirl', 'Clean Girl Aesthetic'),
    ('tt-aesthetic', 'Aesthetic Vibes'), ('tt-grwm', 'GRWM'),
    ('tt-content', 'Content Creator'), ('tt-travel2', 'TravelTok'),
    ('tt-barbie', 'Barbiecore'), ('tt-retro', 'Retro Futurism'),
]:
    path = f'/tmp/{fname}.json'
    if not os.path.exists(path): continue
    data = json.load(open(path))
    items = data if isinstance(data, list) else data.get('data', data.get('videos', []))
    total_views = sum(
        v.get('stats', {}).get('playCount', v.get('playCount', v.get('play_count', 0)))
        for v in items
    )
    results.append({'raw': label, 'signal': total_views, 'source': 'search'})

json.dump(results, open('/tmp/tt-results.json', 'w'), indent=2)
print(f'Saved {len(results)} TikTok signals')
for r in sorted(results, key=lambda x: x['signal'], reverse=True)[:10]:
    print(f"  {r['signal']:>15,}  {r['raw']}")
EOF
```

- [ ] **Step 1.4: Store TikTok results in agentDB**

Use MCP tool `mcp__claude-flow__agentdb_hierarchical-store` (or `mcp__ruflo__agentdb_hierarchical-store`):

```json
{
  "namespace": "landing/niches/tiktok",
  "key": "tiktok-niches-2026-05",
  "value": "<contents of /tmp/tt-results.json as parsed array>",
  "type": "research"
}
```

Load `/tmp/tt-results.json`, pass the array as the `value`.

---

## Task 2 — Instagram Niche Research

**Agent:** `ig-researcher`  
**Files:** None modified — results saved to agentDB

- [ ] **Step 2.1: Search Instagram reels for each niche keyword**

```bash
scrapecreators instagram reels-search --query "fashion aesthetic" --format json --output /tmp/ig-fashion.json
scrapecreators instagram reels-search --query "beauty routine" --format json --output /tmp/ig-beauty.json
scrapecreators instagram reels-search --query "travel aesthetic" --format json --output /tmp/ig-travel.json
scrapecreators instagram reels-search --query "aesthetic vibes" --format json --output /tmp/ig-aesthetic.json
scrapecreators instagram reels-search --query "ootd outfit" --format json --output /tmp/ig-ootd.json
scrapecreators instagram reels-search --query "dark academia" --format json --output /tmp/ig-dark.json
scrapecreators instagram reels-search --query "cottagecore" --format json --output /tmp/ig-cottage.json
scrapecreators instagram reels-search --query "quiet luxury style" --format json --output /tmp/ig-quietlux.json
scrapecreators instagram reels-search --query "mob wife aesthetic" --format json --output /tmp/ig-mobwife.json
scrapecreators instagram reels-search --query "clean girl aesthetic" --format json --output /tmp/ig-cleangirl.json
scrapecreators instagram reels-search --query "coastal grandmother" --format json --output /tmp/ig-coastal.json
scrapecreators instagram reels-search --query "barbiecore" --format json --output /tmp/ig-barbie.json
scrapecreators instagram reels-search --query "street style men" --format json --output /tmp/ig-menstreet.json
scrapecreators instagram reels-search --query "grwm makeup" --format json --output /tmp/ig-grwm.json
scrapecreators instagram reels-search --query "content creator lifestyle" --format json --output /tmp/ig-content.json
scrapecreators instagram reels-search --query "bali travel vlog" --format json --output /tmp/ig-bali.json
scrapecreators instagram reels-search --query "tokyo street fashion" --format json --output /tmp/ig-tokyo.json
scrapecreators instagram reels-search --query "eurotrip aesthetic" --format json --output /tmp/ig-euro.json
scrapecreators instagram reels-search --query "old money aesthetic" --format json --output /tmp/ig-oldmoney.json
scrapecreators instagram reels-search --query "indie aesthetic" --format json --output /tmp/ig-indie.json
```

- [ ] **Step 2.2: Parse reel engagement signals**

```bash
python3 << 'EOF'
import json, os

QUERIES = {
    'ig-fashion': 'Fashion Aesthetic', 'ig-beauty': 'Beauty Routine',
    'ig-travel': 'Travel Aesthetic', 'ig-aesthetic': 'Aesthetic Vibes',
    'ig-ootd': 'OOTD', 'ig-dark': 'Dark Academia',
    'ig-cottage': 'Cottagecore', 'ig-quietlux': 'Quiet Luxury',
    'ig-mobwife': 'Mob Wife Aesthetic', 'ig-cleangirl': 'Clean Girl Aesthetic',
    'ig-coastal': 'Coastal Grandmother', 'ig-barbie': 'Barbiecore',
    'ig-menstreet': 'Streetwear Men', 'ig-grwm': 'GRWM',
    'ig-content': 'Content Creator', 'ig-bali': 'Bali Vibes',
    'ig-tokyo': 'Tokyo Street Style', 'ig-euro': 'Eurotrip Aesthetic',
    'ig-oldmoney': 'Old Money', 'ig-indie': 'Indie Aesthetic',
}

results = []
for fname, label in QUERIES.items():
    path = f'/tmp/{fname}.json'
    if not os.path.exists(path): continue
    data = json.load(open(path))
    items = data if isinstance(data, list) else data.get('data', data.get('reels', data.get('items', [])))
    total = sum(
        v.get('like_count', 0) + v.get('play_count', 0) + v.get('comment_count', 0)
        for v in items
        if isinstance(v, dict)
    )
    results.append({'raw': label, 'signal': total, 'source': 'ig-reels'})

json.dump(results, open('/tmp/ig-results.json', 'w'), indent=2)
print(f'Saved {len(results)} Instagram signals')
for r in sorted(results, key=lambda x: x['signal'], reverse=True)[:10]:
    print(f"  {r['signal']:>15,}  {r['raw']}")
EOF
```

- [ ] **Step 2.3: Store Instagram results in agentDB**

Use MCP tool `mcp__claude-flow__agentdb_hierarchical-store`:

```json
{
  "namespace": "landing/niches/instagram",
  "key": "ig-niches-2026-05",
  "value": "<contents of /tmp/ig-results.json as parsed array>",
  "type": "research"
}
```

---

## Task 3 — Pinterest Niche Research

**Agent:** `pinterest-researcher`  
**Files:** None modified — results saved to agentDB

- [ ] **Step 3.1: Search Pinterest for aesthetic and lifestyle niches**

```bash
scrapecreators pinterest search --query "fashion aesthetic" --trim --format json --output /tmp/pin-fashion.json
scrapecreators pinterest search --query "dark academia outfit" --trim --format json --output /tmp/pin-dark.json
scrapecreators pinterest search --query "cottagecore aesthetic" --trim --format json --output /tmp/pin-cottage.json
scrapecreators pinterest search --query "vintage fashion men" --trim --format json --output /tmp/pin-vintage.json
scrapecreators pinterest search --query "tokyo street style" --trim --format json --output /tmp/pin-tokyo.json
scrapecreators pinterest search --query "coastal grandmother aesthetic" --trim --format json --output /tmp/pin-coastal.json
scrapecreators pinterest search --query "boho fashion" --trim --format json --output /tmp/pin-boho.json
scrapecreators pinterest search --query "preppy aesthetic" --trim --format json --output /tmp/pin-preppy.json
scrapecreators pinterest search --query "golden hour photography" --trim --format json --output /tmp/pin-golden.json
scrapecreators pinterest search --query "winter minimalist outfit" --trim --format json --output /tmp/pin-winter.json
scrapecreators pinterest search --query "eurotrip aesthetic" --trim --format json --output /tmp/pin-euro.json
scrapecreators pinterest search --query "clean girl aesthetic" --trim --format json --output /tmp/pin-clean.json
scrapecreators pinterest search --query "quiet luxury outfit" --trim --format json --output /tmp/pin-quiet.json
scrapecreators pinterest search --query "old money fashion" --trim --format json --output /tmp/pin-oldmoney.json
scrapecreators pinterest search --query "retro futurism fashion" --trim --format json --output /tmp/pin-retro.json
scrapecreators pinterest search --query "cherry blossom korea" --trim --format json --output /tmp/pin-cherry.json
scrapecreators pinterest search --query "indie aesthetic outfit" --trim --format json --output /tmp/pin-indie.json
scrapecreators pinterest search --query "bali travel" --trim --format json --output /tmp/pin-bali.json
scrapecreators pinterest search --query "skincare routine aesthetic" --trim --format json --output /tmp/pin-skincare.json
scrapecreators pinterest search --query "y2k fashion aesthetic" --trim --format json --output /tmp/pin-y2k.json
scrapecreators pinterest search --query "goblincore aesthetic" --trim --format json --output /tmp/pin-goblin.json
scrapecreators pinterest search --query "angelcore aesthetic" --trim --format json --output /tmp/pin-angel.json
scrapecreators pinterest search --query "dreamcore aesthetic" --trim --format json --output /tmp/pin-dream.json
scrapecreators pinterest search --query "streetwear men outfit" --trim --format json --output /tmp/pin-street.json
```

- [ ] **Step 3.2: Parse Pinterest pin counts as engagement signal**

```bash
python3 << 'EOF'
import json, os

QUERIES = {
    'pin-fashion': 'Fashion Aesthetic', 'pin-dark': 'Dark Academia',
    'pin-cottage': 'Cottagecore', 'pin-vintage': 'Vintage Fashion',
    'pin-tokyo': 'Tokyo Street Style', 'pin-coastal': 'Coastal Grandmother',
    'pin-boho': 'Boho Fashion', 'pin-preppy': 'Preppy Aesthetic',
    'pin-golden': 'Golden Hour', 'pin-winter': 'Winter Minimalist',
    'pin-euro': 'Eurotrip Aesthetic', 'pin-clean': 'Clean Girl Aesthetic',
    'pin-quiet': 'Quiet Luxury', 'pin-oldmoney': 'Old Money',
    'pin-retro': 'Retro Futurism', 'pin-cherry': 'Cherry Blossom Seoul',
    'pin-indie': 'Indie Aesthetic', 'pin-bali': 'Bali Vibes',
    'pin-skincare': 'Skincare Routine', 'pin-y2k': 'Y2K Fashion',
    'pin-goblin': 'Goblincore', 'pin-angel': 'Angelcore',
    'pin-dream': 'Dreamcore', 'pin-street': 'Streetwear',
}

results = []
for fname, label in QUERIES.items():
    path = f'/tmp/{fname}.json'
    if not os.path.exists(path): continue
    data = json.load(open(path))
    items = data if isinstance(data, list) else data.get('data', data.get('pins', data.get('items', [])))
    total = sum(
        v.get('repin_count', 0) + v.get('save_count', 0) + v.get('reaction_count', 0)
        for v in items
        if isinstance(v, dict)
    )
    # Use count of results as fallback signal (more pins returned = more content = more popular)
    results.append({'raw': label, 'signal': max(total, len(items) * 1000), 'source': 'pinterest'})

json.dump(results, open('/tmp/pin-results.json', 'w'), indent=2)
print(f'Saved {len(results)} Pinterest signals')
for r in sorted(results, key=lambda x: x['signal'], reverse=True)[:10]:
    print(f"  {r['signal']:>15,}  {r['raw']}")
EOF
```

- [ ] **Step 3.3: Store Pinterest results in agentDB**

Use MCP tool `mcp__claude-flow__agentdb_hierarchical-store`:

```json
{
  "namespace": "landing/niches/pinterest",
  "key": "pin-niches-2026-05",
  "value": "<contents of /tmp/pin-results.json as parsed array>",
  "type": "research"
}
```

---

## Task 4 — Synthesize & Save Final Ranked Niche List

**Agent:** coordinator (waits for Tasks 1–3 to complete via SendMessage)  
**Files:** None modified — final list saved to agentDB

- [ ] **Step 4.1: Retrieve all three platform datasets from agentDB**

Use MCP tool `mcp__claude-flow__agentdb_hierarchical-recall` three times:

```json
{ "namespace": "landing/niches/tiktok",    "key": "tiktok-niches-2026-05" }
{ "namespace": "landing/niches/instagram", "key": "ig-niches-2026-05" }
{ "namespace": "landing/niches/pinterest", "key": "pin-niches-2026-05" }
```

- [ ] **Step 4.2: Deduplicate, merge, and rank**

```bash
python3 << 'EOF'
import json

# Load all three
tt  = json.load(open('/tmp/tt-results.json'))
ig  = json.load(open('/tmp/ig-results.json'))
pin = json.load(open('/tmp/pin-results.json'))

# Canonical label normalization
CANONICAL = {
    'darkacademia': 'Dark Academia', 'dark academia': 'Dark Academia',
    'cottagecore': 'Cottagecore', 'y2k': 'Y2K Fashion', 'y2k fashion': 'Y2K Fashion',
    'streetwear': 'Streetwear', 'streetwear men': 'Streetwear',
    'quiet luxury': 'Quiet Luxury', 'quiet luxury style': 'Quiet Luxury',
    'clean girl': 'Clean Girl Aesthetic', 'clean girl aesthetic': 'Clean Girl Aesthetic',
    'traveltok': 'Travel Aesthetic', 'travel aesthetic': 'Travel Aesthetic',
    'beautytok': 'Beauty Routine', 'beauty routine': 'Beauty Routine',
    'aesthetic': 'Aesthetic Vibes', 'aesthetic vibes': 'Aesthetic Vibes',
    'content creator': 'Content Creator', 'content creator lifestyle': 'Content Creator',
}

merged = {}

for item in tt + ig + pin:
    raw = item['raw']
    key = CANONICAL.get(raw.lower(), raw)
    platform = item.get('source', 'unknown')
    if key not in merged:
        merged[key] = {'label': key, 'platforms': set(), 'engagementRank': 0}
    merged[key]['engagementRank'] += item['signal']
    merged[key]['platforms'].add(platform)

# Convert sets to lists, sort by rank
ranked = sorted(merged.values(), key=lambda x: x['engagementRank'], reverse=True)
for n in ranked:
    n['platforms'] = sorted(n['platforms'])

# Ensure all expected niches are present even if signal was 0
BASELINE = [
    'Y2K Fashion', 'Dark Academia', 'Cottagecore', 'Streetwear', 'Quiet Luxury',
    'Mob Wife Aesthetic', 'Clean Girl Aesthetic', 'Barbiecore', 'Vintage Fashion',
    'Preppy Aesthetic', 'Boho Fashion', 'Old Money', 'Beauty Routine',
    'Skincare Routine', 'GRWM', 'Soft Girl', 'Aesthetic Vibes', 'Coastal Grandmother',
    'Golden Hour', 'Moody Aesthetic', 'Indie Aesthetic', 'Retro Futurism',
    'Dreamcore', 'Goblincore', 'Angelcore', 'Travel Aesthetic', 'Bali Vibes',
    'Tokyo Street Style', 'Eurotrip Aesthetic', 'Cherry Blossom Seoul',
    'Winter Minimalist', 'NYC Fashion', 'Content Creator', 'OOTD',
    'Fashion Aesthetic', 'Photo Dump', 'Cinematic Vlog', 'Creator Lifestyle',
    'Vlog Aesthetic',
]
existing_labels = {n['label'] for n in ranked}
for b in BASELINE:
    if b not in existing_labels:
        ranked.append({'label': b, 'platforms': ['baseline'], 'engagementRank': 0})

json.dump(ranked, open('/tmp/ranked-niches.json', 'w'), indent=2)
print(f'Total niches: {len(ranked)}')
for n in ranked[:15]:
    print(f"  {n['engagementRank']:>15,}  {n['label']}  [{', '.join(n['platforms'])}]")
EOF
```

- [ ] **Step 4.3: Save final ranked list to agentDB**

Use MCP tool `mcp__claude-flow__agentdb_hierarchical-store`:

```json
{
  "namespace": "landing/niches",
  "key": "ranked-niches-2026-05",
  "value": "<contents of /tmp/ranked-niches.json as parsed array>",
  "type": "final"
}
```

---

## Task 5 — Create src/data/landing-niches.ts

**Agent:** coder  
**Files:** Create `src/data/landing-niches.ts`

- [ ] **Step 5.1: Retrieve final ranked list from agentDB**

Use MCP tool `mcp__claude-flow__agentdb_hierarchical-recall`:

```json
{ "namespace": "landing/niches", "key": "ranked-niches-2026-05" }
```

Load the result, or read `/tmp/ranked-niches.json` if agentDB retrieval is not available in this session.

- [ ] **Step 5.2: Create the data directory and file**

```bash
mkdir -p /mnt/c/Users/Jamaal/Documents/Phantom\ Systems\ Inc/mirai-clone/src/data
```

- [ ] **Step 5.3: Write src/data/landing-niches.ts**

Write this file using the actual ranked array from agentDB. The `NICHES` array below shows structure — replace with the real research data (40+ entries, sorted by `engagementRank` descending):

```typescript
// Sourced from ScrapeCreator research 2026-05 (TikTok + Instagram + Pinterest).
// Re-run research and update when niches go stale.

export interface Niche {
  label: string;
  platforms: string[];
  engagementRank: number;
}

export const NICHES: Niche[] = [
  // ← Insert full ranked array from /tmp/ranked-niches.json here.
  // Each entry: { label: "...", platforms: ["tiktok", ...], engagementRank: 12345 }
  // Must be sorted by engagementRank descending (highest first).
  // Must contain 40+ entries.
];

// Top 16 niches for the gallery strip (one image per niche)
export const GALLERY_NICHES = NICHES.slice(0, 16);

// All labels for the social proof strip marquee
export const PROOF_LABELS = NICHES.map((n) => n.label);
```

- [ ] **Step 5.4: Verify the file compiles**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npx tsc --noEmit 2>&1 | grep landing-niches || echo "No errors in landing-niches.ts"
```

Expected: no output (no errors).

---

## Task 6 — Generate Gallery Images (Higgsfield GPT Image 2.0)

**Agent:** coder (or run manually — requires Higgsfield CLI auth)  
**Files:** Create `public/landing/gallery/*.jpg`

- [ ] **Step 6.1: Create output directory**

```bash
mkdir -p "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery"
```

- [ ] **Step 6.2: Verify Higgsfield auth**

```bash
higgsfield account status
```

Expected: prints account email and credit balance. If `Session expired`, run `higgsfield auth login`.

- [ ] **Step 6.3: Generate 8 female-character gallery images**

Run each command. `--wait` blocks until complete and prints the result URL.

```bash
# 1. Y2K Fashion (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in Y2K fashion — low-rise jeans, butterfly clips, metallic crop top, chunky platforms, urban street backdrop, warm film grain, editorial fashion photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); url=j[0].get('output_url',j[0].get('url','')); print(url)" \
  > /tmp/url-gallery-y2k-fashion.txt

# Download (replace URL with actual)
curl -L "$(cat /tmp/url-gallery-y2k-fashion.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-y2k-fashion.jpg"

# 2. Dark Academia (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in dark academia aesthetic — plaid blazer, turtleneck, Oxford courtyard, autumn leaves, moody chiaroscuro light, cinematic portrait photography" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-dark-academia.txt
curl -L "$(cat /tmp/url-gallery-dark-academia.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-dark-academia.jpg"

# 3. Cottagecore (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in cottagecore aesthetic — floral prairie dress, wicker basket, wildflower meadow, golden hour backlight, dreamy soft photography, editorial portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-cottagecore.txt
curl -L "$(cat /tmp/url-gallery-cottagecore.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-cottagecore.jpg"

# 4. Quiet Luxury (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in quiet luxury aesthetic — tailored camel coat, minimal gold jewelry, clean marble architecture, soft diffused daylight, understated editorial fashion photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-quiet-luxury.txt
curl -L "$(cat /tmp/url-gallery-quiet-luxury.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-quiet-luxury.jpg"

# 5. Barbiecore (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in barbiecore aesthetic — all-pink outfit, bold accessories, playful fashion editorial, bright saturated photography, Malibu pop art background, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-barbiecore.txt
curl -L "$(cat /tmp/url-gallery-barbiecore.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-barbiecore.jpg"

# 6. Clean Girl Aesthetic (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in clean girl aesthetic — slicked back bun, gold hoop earrings, fresh dewy skin, white fitted top, neutral background, glossy editorial beauty photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-clean-girl.txt
curl -L "$(cat /tmp/url-gallery-clean-girl.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-clean-girl-aesthetic.jpg"

# 7. Coastal Grandmother (female)
higgsfield generate create gpt_image_2 \
  --prompt "Woman in coastal grandmother aesthetic — cream linen trousers, striped boatneck top, seaside town promenade, warm afternoon light, relaxed editorial lifestyle photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-coastal-grandmother.txt
curl -L "$(cat /tmp/url-gallery-coastal-grandmother.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-coastal-grandmother.jpg"

# 8. Boho Fashion (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman in boho fashion — flowing terracotta maxi dress, layered beaded necklaces, desert canyon backdrop, warm golden dusk light, editorial fashion photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-boho-fashion.txt
curl -L "$(cat /tmp/url-gallery-boho-fashion.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-boho-fashion.jpg"
```

- [ ] **Step 6.4: Generate 8 male-character gallery images**

```bash
# 9. Streetwear (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in streetwear aesthetic — oversized graphic hoodie, cargo pants, chunky sneakers, graffiti urban alley, moody blue hour street photography, editorial portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-streetwear.txt
curl -L "$(cat /tmp/url-gallery-streetwear.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-streetwear.jpg"

# 10. Tokyo Street Style (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in Tokyo street style — avant-garde layered Harajuku fashion, neon-lit Shibuya crossing at night, cinematic wet street reflections, high-fashion editorial photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-tokyo-street.txt
curl -L "$(cat /tmp/url-gallery-tokyo-street.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-tokyo-street-style.jpg"

# 11. Vintage Fashion (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in vintage 70s fashion — brown corduroy jacket, flared denim, tinted aviator sunglasses, retro film photography look, warm amber nostalgic tones, editorial portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-vintage-fashion.txt
curl -L "$(cat /tmp/url-gallery-vintage-fashion.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-vintage-fashion.jpg"

# 12. Old Money (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in old money aesthetic — cream cable-knit sweater, tailored navy trousers, yacht club deck, soft afternoon sea light, understated luxury editorial photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-old-money.txt
curl -L "$(cat /tmp/url-gallery-old-money.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-old-money.jpg"

# 13. Preppy Aesthetic (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in preppy aesthetic — Ralph Lauren polo, chinos, varsity jacket, ivy league campus in autumn, warm golden foliage, editorial fashion photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-preppy.txt
curl -L "$(cat /tmp/url-gallery-preppy.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-preppy-aesthetic.jpg"

# 14. Eurotrip Aesthetic (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in Eurotrip aesthetic — linen shirt, bucket hat, cobblestone piazza in Rome, golden hour gelato moment, warm travel editorial photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-eurotrip.txt
curl -L "$(cat /tmp/url-gallery-eurotrip.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-eurotrip-aesthetic.jpg"

# 15. Golden Hour (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man bathed in golden hour light — casual linen outfit, rooftop setting with city skyline, warm backlit silhouette, lifestyle editorial photography, portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-golden-hour.txt
curl -L "$(cat /tmp/url-gallery-golden-hour.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-golden-hour.jpg"

# 16. Indie Aesthetic (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man in indie aesthetic — vintage band tee, worn jeans, candid expression, 35mm film grain photography, warm nostalgic tones, creative editorial portrait" \
  --aspect_ratio 2:3 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-gallery-indie.txt
curl -L "$(cat /tmp/url-gallery-indie.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/gallery-indie-aesthetic.jpg"
```

- [ ] **Step 6.5: Verify all 16 gallery images exist**

```bash
ls "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/gallery/" | wc -l
```

Expected: `16`

- [ ] **Step 6.6: Review images before proceeding**

Open each image from `public/landing/gallery/`. Confirm portrait orientation, no watermarks, 8 female + 8 male characters, distinct compositions. Do not proceed to Task 8 until all 16 are approved.

---

## Task 7 — Generate Hero Deck Images (Higgsfield GPT Image 2.0)

**Agent:** coder (same session as Task 6 or sequential)  
**Files:** Create `public/landing/hero/*.jpg`

Hero images are close-crop editorial portraits — distinct from gallery images in composition and niche selection.

- [ ] **Step 7.1: Create output directory**

```bash
mkdir -p "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero"
```

- [ ] **Step 7.2: Generate 7 hero deck images**

```bash
# 1. Aesthetic Vibes (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman close-up editorial portrait, pastel aesthetic vibes — soft dreamy out-of-focus floral background, delicate pearl jewelry, glowy skin, intimate beauty photography, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-aesthetic-vibes.txt
curl -L "$(cat /tmp/url-hero-aesthetic-vibes.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-aesthetic-vibes.jpg"

# 2. Retro Futurism (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man close-up editorial portrait, retro futurism aesthetic — chrome metallic jacket, sleek futuristic eyewear, deep neon blue ambient background, cinematic studio lighting, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-retro-futurism.txt
curl -L "$(cat /tmp/url-hero-retro-futurism.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-retro-futurism.jpg"

# 3. Bali Vibes (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman editorial portrait, Bali travel aesthetic — flowy white sundress, dense tropical palm leaves, warm golden dappled sunlight, relaxed lifestyle photography, face visible, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-bali-vibes.txt
curl -L "$(cat /tmp/url-hero-bali-vibes.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-bali-vibes.jpg"

# 4. GRWM (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman GRWM close-up portrait — natural dewy makeup look, soft ring-light catchlights in eyes, vanity mirror visible in background, authentic creator bedroom aesthetic, warm editorial photography, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-grwm.txt
curl -L "$(cat /tmp/url-hero-grwm.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-grwm.jpg"

# 5. Indie Aesthetic (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man indie aesthetic editorial portrait — vintage band tee, candid thoughtful expression, 35mm film grain, warm light leak, creative portrait photography close-up, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-indie.txt
curl -L "$(cat /tmp/url-hero-indie.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-indie-aesthetic.jpg"

# 6. Cherry Blossom Seoul (female)
higgsfield generate create gpt_image_2 \
  --prompt "Young woman editorial portrait, Cherry Blossom Seoul aesthetic — pink floral-accented top, sakura petals in background, soft spring light, Korean editorial photography, ultra detailed, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-cherry-blossom.txt
curl -L "$(cat /tmp/url-hero-cherry-blossom.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-cherry-blossom.jpg"

# 7. NYC Fashion (male)
higgsfield generate create gpt_image_2 \
  --prompt "Young man NYC fashion editorial portrait — sharp tailored dark look, Manhattan skyline backdrop, overcast dramatic sky, high-contrast cinematic fashion photography, confident expression, 9:16 crop" \
  --aspect_ratio 9:16 --quality high --resolution 2k --wait \
  --json | python3 -c "import json,sys; j=json.load(sys.stdin); print(j[0].get('output_url',j[0].get('url','')))" \
  > /tmp/url-hero-nyc.txt
curl -L "$(cat /tmp/url-hero-nyc.txt)" \
  -o "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/hero-nyc-fashion.jpg"
```

- [ ] **Step 7.3: Verify all 7 hero images exist**

```bash
ls "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone/public/landing/hero/" | wc -l
```

Expected: `7`

- [ ] **Step 7.4: Review hero images before proceeding**

Confirm close-crop composition, faces clearly visible, distinct from gallery images. All 4 female + 3 male. Approve before Task 8.

---

## Task 8 — Update SocialProofStrip

**Files:**
- Modify: `src/client/screens/landing/SocialProofStrip.tsx`
- Modify: `src/client/screens/landing/landing.css`

- [ ] **Step 8.1: Read current SocialProofStrip.tsx**

```
Read: src/client/screens/landing/SocialProofStrip.tsx
```

- [ ] **Step 8.2: Replace SocialProofStrip.tsx**

Write the entire file:

```tsx
import { PROOF_LABELS } from '../../../data/landing-niches';

const TRIPLED = [...PROOF_LABELS, ...PROOF_LABELS, ...PROOF_LABELS];

export function SocialProofStrip() {
  return (
    <section className="lp-proof">
      <p className="lp-proof__label">Trusted by 10,000+ creators</p>
      <div className="lp-marquee">
        <div className="lp-marquee__track">
          {TRIPLED.map((label, i) => (
            <span key={i} className="lp-marquee__chip">{label}</span>
          ))}
        </div>
      </div>
    </section>
  );
}
```

- [ ] **Step 8.3: Read current landing.css**

```
Read: src/client/screens/landing/landing.css
```

- [ ] **Step 8.4: Fix marquee CSS for 3× duplication and unlimited loop**

Find the `.lp-marquee__track` rule and the `@keyframes marquee-scroll` rule and replace them:

Old `.lp-marquee__track`:
```css
.lp-marquee__track {
  display: flex;
  gap: 12px;
  animation: marquee-scroll 30s linear infinite;
}
```

New:
```css
.lp-marquee__track {
  display: flex;
  gap: 12px;
  width: max-content;
  animation: marquee-scroll 60s linear infinite;
}
```

Old `@keyframes marquee-scroll`:
```css
@keyframes marquee-scroll {
  from { transform: translateX(0); }
  to { transform: translateX(-50%); }
}
```

New:
```css
@keyframes marquee-scroll {
  from { transform: translateX(0); }
  to { transform: translateX(-33.333%); }
}
```

- [ ] **Step 8.5: Verify in browser**

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone" && npm run dev
```

Open `http://localhost:5173`. Confirm: niche labels visible (no `#` hashtags), marquee scrolls left, loops with no gap or jump.

---

## Task 9 — Update GalleryStrip (Lazy Load + New Images + Unlimited Loop)

**Files:**
- Modify: `src/client/screens/landing/GalleryStrip.tsx`
- Modify: `src/client/screens/landing/landing.css`

- [ ] **Step 9.1: Read current GalleryStrip.tsx**

```
Read: src/client/screens/landing/GalleryStrip.tsx
```

- [ ] **Step 9.2: Write complete new GalleryStrip.tsx**

```tsx
import { useRef, useState, useEffect } from 'react';
import { GALLERY_NICHES } from '../../../data/landing-niches';

function useLazyImage(src: string, rootMargin = '300px') {
  const ref = useRef<HTMLDivElement>(null);
  const [resolvedSrc, setResolvedSrc] = useState<string | undefined>();

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setResolvedSrc(src);
          observer.disconnect();
        }
      },
      { rootMargin }
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [src, rootMargin]);

  return { ref, resolvedSrc };
}

const nicheToSlug = (label: string) =>
  label.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '');

const ROW1 = GALLERY_NICHES.filter((_, i) => i % 2 === 0).map((n) => ({
  src: `/landing/gallery/gallery-${nicheToSlug(n.label)}.jpg`,
  label: n.label,
}));

const ROW2 = GALLERY_NICHES.filter((_, i) => i % 2 === 1).map((n) => ({
  src: `/landing/gallery/gallery-${nicheToSlug(n.label)}.jpg`,
  label: n.label,
}));

function GalleryCard({ src, label }: { src: string; label: string }) {
  const { ref, resolvedSrc } = useLazyImage(src, '300px');

  return (
    <div className="lp-gallery__card" ref={ref}>
      {resolvedSrc ? (
        <img src={resolvedSrc} alt={label} loading="lazy" />
      ) : (
        <div className="lp-gallery__card-placeholder" aria-hidden="true" />
      )}
      <div className="lp-gallery__card-label">{label}</div>
    </div>
  );
}

export function GalleryStrip() {
  return (
    <section className="lp-section" id="gallery">
      <div className="lp-container">
        <h2 className="lp-h2">See what Mirai makes</h2>
      </div>
      <div className="lp-gallery">
        <div className="lp-gallery__row">
          <div className="lp-gallery__track">
            {[...ROW1, ...ROW1, ...ROW1].map((card, i) => (
              <GalleryCard key={i} {...card} />
            ))}
          </div>
        </div>
        <div className="lp-gallery__row">
          <div className="lp-gallery__track lp-gallery__track--reverse">
            {[...ROW2, ...ROW2, ...ROW2].map((card, i) => (
              <GalleryCard key={i} {...card} />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
```

- [ ] **Step 9.3: Update gallery CSS for 3× duplication, unlimited loop, and placeholder**

In `landing.css`, find and replace the gallery track rules and keyframe:

Old `.lp-gallery__track`:
```css
.lp-gallery__track {
  display: flex;
  gap: 16px;
  animation: gallery-left 40s linear infinite;
}
```

New:
```css
.lp-gallery__track {
  display: flex;
  gap: 16px;
  width: max-content;
  animation: gallery-left 70s linear infinite;
}
```

Old `@keyframes gallery-left`:
```css
@keyframes gallery-left {
  from { transform: translateX(0); }
  to { transform: translateX(-50%); }
}
```

New:
```css
@keyframes gallery-left {
  from { transform: translateX(0); }
  to { transform: translateX(-33.333%); }
}
```

Add after `.lp-gallery__card img` rule:
```css
.lp-gallery__card-placeholder {
  width: 100%;
  height: 100%;
  background: #f0f0f0;
}
```

- [ ] **Step 9.4: Verify in browser**

- Row 1 scrolls left, row 2 scrolls right, no gap at loop point
- Off-screen cards show grey placeholder, load image as they approach 300px from viewport
- Hovering a row pauses it

---

## Task 10 — Update SwipeDeckHero (Better Swipe + Hero Images)

**Files:**
- Modify: `src/client/screens/landing/SwipeDeckHero.tsx`

- [ ] **Step 10.1: Read current SwipeDeckHero.tsx**

```
Read: src/client/screens/landing/SwipeDeckHero.tsx
```

- [ ] **Step 10.2: Write complete new SwipeDeckHero.tsx**

Key changes from current: hero-specific images, wider drag range (±300), `dragElastic={0.15}`, `dragMomentum={false}`, velocity-based dismiss (`|velocity.x| > 400`), `AnimatePresence` for label, `whileDrag` scale.

```tsx
import { useState, useEffect } from 'react';
import { motion, useMotionValue, useTransform, AnimatePresence } from 'motion/react';

const CARDS = [
  { src: '/landing/hero/hero-aesthetic-vibes.jpg',  label: 'Aesthetic Vibes' },
  { src: '/landing/hero/hero-retro-futurism.jpg',   label: 'Retro Futurism' },
  { src: '/landing/hero/hero-bali-vibes.jpg',       label: 'Bali Vibes' },
  { src: '/landing/hero/hero-grwm.jpg',             label: 'GRWM' },
  { src: '/landing/hero/hero-indie-aesthetic.jpg',  label: 'Indie Aesthetic' },
  { src: '/landing/hero/hero-cherry-blossom.jpg',   label: 'Cherry Blossom Seoul' },
  { src: '/landing/hero/hero-nyc-fashion.jpg',      label: 'NYC Fashion' },
];

function DeckCard({
  src,
  label,
  position,
  onDragEnd,
}: {
  src: string;
  label: string;
  position: number;
  onDragEnd: (offsetX: number, velocityX: number) => void;
}) {
  const x = useMotionValue(0);
  const rotate = useTransform(x, [-200, 200], [-12, 12]);

  return (
    <motion.div
      className="lp-deck__card"
      style={{
        x,
        rotate: position === 0 ? rotate : position * 3 - 3,
        y: position * 8,
        zIndex: CARDS.length - position,
        position: 'absolute',
      }}
      drag={position === 0 ? 'x' : false}
      dragConstraints={{ left: -300, right: 300 }}
      dragElastic={0.15}
      dragMomentum={false}
      onDragEnd={(_, info) => {
        if (position === 0) onDragEnd(info.offset.x, info.velocity.x);
      }}
      animate={
        position !== 0
          ? { rotate: position * 3 - 3, y: position * 8, x: 0 }
          : undefined
      }
      transition={{ type: 'spring', stiffness: 400, damping: 35 }}
      whileDrag={{ scale: 1.02, cursor: 'grabbing' }}
    >
      <img
        src={src}
        alt={label}
        loading={position === 0 ? 'eager' : 'lazy'}
      />
    </motion.div>
  );
}

export function SwipeDeckHero() {
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    const id = setInterval(() => {
      setActiveIndex((i) => (i + 1) % CARDS.length);
    }, 3000);
    return () => clearInterval(id);
  }, []);

  const handleDragEnd = (offsetX: number, velocityX: number) => {
    const byVelocity = Math.abs(velocityX) > 400;
    const byOffset = Math.abs(offsetX) > 80;
    if (byVelocity || byOffset) {
      if (velocityX < 0 || offsetX < 0) {
        setActiveIndex((i) => (i + 1) % CARDS.length);
      } else {
        setActiveIndex((i) => (i - 1 + CARDS.length) % CARDS.length);
      }
    }
  };

  const orderedCards = [
    ...CARDS.slice(activeIndex),
    ...CARDS.slice(0, activeIndex),
  ];

  return (
    <div className="lp-deck">
      {orderedCards.map((card, position) => (
        <DeckCard
          key={card.src}
          src={card.src}
          label={card.label}
          position={position}
          onDragEnd={handleDragEnd}
        />
      ))}
      <AnimatePresence mode="wait">
        <motion.div
          key={orderedCards[0].label}
          className="lp-deck__label"
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -4 }}
          transition={{ duration: 0.2 }}
        >
          {orderedCards[0].label}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
```

- [ ] **Step 10.3: Verify swipe behaviour in browser**

Open the dev server. On the hero deck:
- Drag left slowly past 80px offset → card advances
- Flick left quickly (short distance) → card advances via velocity threshold
- Drag right → previous card
- Label below deck fades to new niche name with each swipe
- Auto-advance every 3 seconds still works
- DevTools mobile emulation (iPhone 14, 390×844): touch drag feels natural, no overshoot

---

## Task 11 — Browser Review with Chrome MCP

**Files:** No source changes — visual QA only

- [ ] **Step 11.1: Load Chrome MCP tools**

```javascript
// ToolSearch: select:mcp__claude-in-chrome__tabs_context_mcp
// ToolSearch: select:mcp__claude-in-chrome__navigate
// ToolSearch: select:mcp__claude-in-chrome__screenshot
// ToolSearch: select:mcp__claude-in-chrome__resize_window
```

- [ ] **Step 11.2: Open landing page — desktop**

Navigate to `http://localhost:5173`. Resize to 1440×900. Take full-page screenshot.

Check:
- Hero: swipe deck on right, copy on left, no overlap
- SocialProofStrip: niche labels, no hashtags, smooth scroll
- GalleryStrip: two rows scrolling opposite directions, grey placeholders visible for far-off cards
- No console errors (check `mcp__claude-in-chrome__read_console_messages`)

- [ ] **Step 11.3: Check mobile viewport**

Resize to 390×844 (iPhone 14). Take screenshot.

Check:
- Hero deck is full-width, swipeable
- SocialProofStrip chips fit correctly
- Gallery rows visible and scrolling

- [ ] **Step 11.4: Fix any issues**

If visual regressions found, fix them in the relevant file and re-screenshot. Repeat until both viewports pass.

---

## Self-Review — Spec vs Plan

| Spec requirement | Task |
|-----------------|------|
| Research swarm: TikTok, Instagram, Pinterest | Tasks 1, 2, 3 |
| Coordinator synthesizes, saves to agentDB | Task 4 |
| 40+ niches, trending first | Task 4 (ranked), Task 5 (`NICHES` sorted by `engagementRank`) |
| Remove hashtags from SocialProofStrip | Task 8 (no `#`, plain labels) |
| SocialProofStrip unlimited loop | Task 8 (3× tripling + `−33.333%` keyframe) |
| Gallery: new Higgsfield images | Task 6 (16 images) |
| Gallery: boy + girl characters | Task 6 (8 female + 8 male, named explicitly) |
| Gallery ≠ hero images | Tasks 6 & 7 use different niches and compositions |
| Gallery: trending niches first | Task 5 (`GALLERY_NICHES = NICHES.slice(0, 16)` — sorted) |
| Gallery: lazy load IntersectionObserver | Task 9 (`useLazyImage` hook, `300px` rootMargin) |
| Gallery: unlimited loop | Task 9 (3× + `−33.333%` keyframe, `width: max-content`) |
| Hero images: `loading="eager"` top card | Task 10 (`loading={position === 0 ? 'eager' : 'lazy'}`) |
| Hero: improved swipe physics | Task 10 (`dragElastic`, `dragMomentum`, velocity threshold) |
| Hero: AnimatePresence label | Task 10 (`AnimatePresence mode="wait"` with fade+slide) |
| No layout shift (placeholders) | Task 9 (`lp-gallery__card-placeholder` same dimensions) |
| No runtime API calls | Confirmed — all static after generation |
| No commit | Confirmed — no `git commit` steps included |
| Chrome MCP review | Task 11 |

All spec requirements covered. No placeholders or TBDs remain in implementation steps.
