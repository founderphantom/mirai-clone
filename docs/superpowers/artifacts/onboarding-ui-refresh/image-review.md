# Higgsfield Image Review

Date: 2026-05-07

## Inputs

- Current `/onboarding` mobile and desktop screenshots
- Current `/me` mobile and desktop screenshots
- Current landing mobile and desktop screenshots
- GPT Image 2 model schema from `higgsfield model get gpt_image_2 --json`

## UI Inspiration

- `public/landing/ui-inspiration/onboarding/onboarding-mobile.png`: pass. Strong mobile composition, dark mint surface language, image-led source cards, and centered aesthetic bubbles. Use as inspiration only; do not copy generated mock text literally.
- `public/landing/ui-inspiration/onboarding/onboarding-desktop.png`: pass. Practical desktop structure with account/progress context, step sections, and compact bubble choices. Use as layout inspiration while preserving existing app behavior.

## Bubble Assets

- `public/landing/onboarding-bubbles/bubble-beauty.png`: pass. Product/skincare composition, no text, generous center space for label overlay.
- `public/landing/onboarding-bubbles/bubble-travel.png`: pass. Warm travel room/view composition, no text, works with dark text scrim.
- `public/landing/onboarding-bubbles/bubble-fashion.png`: pass. Object-only fashion/material detail after safer regeneration; no people, no text, circular crop ready.
- `public/landing/onboarding-bubbles/bubble-vibes.png`: pass. Night/content mood board with dark center, no text, strong fit for cinematic/y2k/nightlife bubbles.
- `public/landing/onboarding-bubbles/bubble-content.png`: pass. Creator gear composition, no text, dark center supports readable overlay.
- `public/landing/onboarding-bubbles/bubble-wellness.png`: pass. Wellness/fitness flat lay, no text, ample center space.

## Usage Notes

- Apply a dark overlay/scrim in CSS rather than editing the image files.
- Keep bubble text centered in the circle, as requested.
- Use image mapping by slug/title/vibe keywords so backend data does not need to change.
- Preserve accessible button labels and `aria-pressed` states.
