# Ruflo Execution Plan - Onboarding UI/UX Refresh

Date: 2026-05-07
Status: Draft for approval
Spec: `docs/superpowers/specs/2026-05-07-onboarding-ui-ux-refresh.md`
Primary executor: Ruflo CLI, because the Ruflo MCP tool transport became unavailable in this Codex session after initial discovery.
Required capabilities: `swarm-orchestration`, `memory-management`, `agent-coder`, `agent-tester`, `agent-reviewer`.

Do not start this plan until the user approves it.

## Orchestration Readiness

Verified:

- `tool_search` discovered Ruflo MCP tools.
- Initial Ruflo MCP status returned running over stdio.
- Later Ruflo MCP calls failed with `Transport closed`; use Ruflo CLI as the primary execution path unless MCP is reconnected.
- `npx -y ruflo@latest --version` returned `ruflo v3.7.0-alpha.11`.
- `npx -y ruflo@latest mcp status --format json` reported a stdio MCP process running.
- Repo config confirms:
  - model: `deepseek-v4-pro`
  - baseUrl: `https://opencode.ai/zen/go/v1`
- Ruflo CLI memory search for Mirai design tokens returned no entries; use project docs and live screenshots as design-token sources.

Current repo caveat: the worktree is already dirty with unrelated landing image/code changes and deleted `.claude/skills` files. Ruflo agents must not revert unrelated changes.

## File Map

Read before editing:

- `AGENTS.md`
- `CLAUDE.md`
- `.mcp.json`
- `claude-flow.config.json`
- `package.json`
- `docs/mirai-2026-redesign-review.md`
- `docs/superpowers/specs/2026-05-06-app-ui-ux-refresh-design.md`
- `docs/superpowers/plans/2026-05-06-app-ui-ux-refresh.md`
- `src/client/screens/OnboardingScreen.tsx`
- `src/client/screens/MeScreen.tsx`
- `src/client/app-ui.css`
- `src/client/mobile.css`
- `src/client/main.tsx`
- `src/client/types.ts`
- `src/server/routes/onboarding.ts`
- `src/server/services/persona-agent.ts`

Expected source writes:

- `src/client/screens/OnboardingScreen.tsx`
- `src/client/screens/onboarding-visuals.ts`
- `src/client/onboarding.css`
- `src/client/main.tsx`
- `tests/client/onboarding-visuals.test.ts`

Expected asset writes:

- `public/landing/ui-inspiration/onboarding/onboarding-mobile.jpg`
- `public/landing/ui-inspiration/onboarding/onboarding-desktop.jpg`
- `public/landing/ui-inspiration/onboarding/bubbles-mobile.jpg`
- `public/landing/ui-inspiration/onboarding/source-picker-desktop.jpg`
- `public/landing/onboarding-bubbles/bubble-fashion.jpg`
- `public/landing/onboarding-bubbles/bubble-beauty.jpg`
- `public/landing/onboarding-bubbles/bubble-vibes.jpg`
- `public/landing/onboarding-bubbles/bubble-travel.jpg`
- `public/landing/onboarding-bubbles/bubble-content.jpg`
- `public/landing/onboarding-bubbles/bubble-wellness.jpg`
- `public/landing/onboarding-bubbles/bubble-streetwear.jpg`
- `public/landing/onboarding-bubbles/bubble-editorial.jpg`

Expected artifact writes:

- `docs/superpowers/artifacts/onboarding-ui-refresh/current-onboarding-mobile.png`
- `docs/superpowers/artifacts/onboarding-ui-refresh/current-onboarding-desktop.png`
- `docs/superpowers/artifacts/onboarding-ui-refresh/current-me-mobile.png`
- `docs/superpowers/artifacts/onboarding-ui-refresh/current-me-desktop.png`
- `docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-mobile.png`
- `docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-desktop.png`
- `docs/superpowers/artifacts/onboarding-ui-refresh/image-review.md`
- `docs/superpowers/artifacts/onboarding-ui-refresh/browser-review.md`

Do not write credentials to docs or source.

## Task Dependency Graph

```text
T0 approval
  -> T1 readiness + memory/context search
  -> T2 baseline screenshots
  -> T3 Higgsfield GPT Image 2 generation
  -> T4 visual image review gate
  -> T5 source implementation
  -> T6 unit tests
  -> T7 local validation
  -> T8 browser-use/mobile-desktop review
  -> T9 reviewer pass
  -> T10 memory store + completion report
```

Blocking rules:

- T3 must not start until T2 screenshots exist.
- T5 must not start until T4 approves exact image filenames.
- T8 must not start until T7 passes.
- T10 must not run until reviewer approves.

## Ruflo CLI Commands

Run from repo root:

```bash
cd "/mnt/c/Users/Jamaal/Documents/Phantom Systems Inc/mirai-clone"
```

Readiness:

```bash
npx -y ruflo@latest --version
npx -y ruflo@latest config get agents.providers --format json
npx -y ruflo@latest mcp status --format json
npx -y ruflo@latest providers list --format json
npx -y ruflo@latest memory search -q "Mirai onboarding UI design tokens /me page Higgsfield GPT Image 2 bubble visuals" -n patterns -l 10 --smart --format json
npx -y ruflo@latest hooks route -t "Improve Mirai /onboarding UI/UX with Higgsfield GPT Image 2 inspiration, image bubble picker, mobile and desktop browser review" -K 5
```

Initialize swarm:

```bash
npx -y ruflo@latest swarm init --topology hierarchical --max-agents 8 --strategy specialized
```

Spawn agents:

```bash
npx -y ruflo@latest agent spawn -t swarm-memory-manager -n onboarding-memory --provider openai --model deepseek-v4-pro --task "Read docs/superpowers/specs/2026-05-07-onboarding-ui-ux-refresh.md and docs/superpowers/plans/2026-05-07-onboarding-ui-ux-refresh.md. Search memory for relevant Mirai design/UI patterns. Do not edit source. Store final learnings only after reviewer approval."

npx -y ruflo@latest agent spawn -t coder -n onboarding-assets-coder --provider openai --model deepseek-v4-pro --timeout 7200 --task "Read only the saved spec and plan. Own T2-T4 asset workflow only. Write only public/landing/ui-inspiration/onboarding, public/landing/onboarding-bubbles, and docs/superpowers/artifacts/onboarding-ui-refresh. Use Higgsfield CLI gpt_image_2 with current screenshots as image inputs. Do not edit src or tests."

npx -y ruflo@latest agent spawn -t coder -n onboarding-ui-coder --provider openai --model deepseek-v4-pro --timeout 7200 --task "Read only the saved spec and plan plus the exact source files named in the file map. Own T5. Write only src/client/screens/OnboardingScreen.tsx, src/client/screens/onboarding-visuals.ts, src/client/onboarding.css, and src/client/main.tsx. Do not touch server code, generated images, or unrelated screens."

npx -y ruflo@latest agent spawn -t tester -n onboarding-tester --provider openai --model deepseek-v4-pro --timeout 7200 --task "Read the saved spec and plan. Own T6-T8. Write only tests/client/onboarding-visuals.test.ts and docs/superpowers/artifacts/onboarding-ui-refresh/browser-review.md unless explicitly coordinating a small test-only fix. Run typecheck, tests, build, and browser-use review."

npx -y ruflo@latest agent spawn -t reviewer -n onboarding-reviewer --provider openai --model deepseek-v4-pro --timeout 7200 --task "Read the saved spec and plan. Own T9. Review only the diff and validation artifacts. Focus on regressions, accessibility, image usage, token waste, security, and file ownership. Do not edit files."
```

Start coordinated execution after all agents exist:

```bash
npx -y ruflo@latest swarm start -o "Execute docs/superpowers/plans/2026-05-07-onboarding-ui-ux-refresh.md through the dependency graph. Stop at each blocking gate. Do not modify files outside declared ownership." -s development --parallel
```

Optional MCP equivalents if Ruflo MCP reconnects:

```text
mcp__ruflo__.agent_spawn({ agentType: "coder", agentId: "onboarding-ui-coder", domain: "frontend", model: "inherit", task: "Read docs/superpowers/plans/2026-05-07-onboarding-ui-ux-refresh.md and own T5 only." })
mcp__ruflo__.coordination_orchestrate({ strategy: "pipeline", agents: ["onboarding-assets-coder", "onboarding-ui-coder", "onboarding-tester", "onboarding-reviewer"], task: "Execute docs/superpowers/plans/2026-05-07-onboarding-ui-ux-refresh.md with blocking gates." })
mcp__ruflo__.agentdb_hierarchical_store({ tier: "semantic", key: "mirai/onboarding-ui-refresh/2026-05-07", value: "Final reviewed implementation notes after completion." })
```

## Agent Responsibilities

### onboarding-memory

Ownership:

- Memory search before implementation.
- Final memory store after reviewer approval.

Commands:

```bash
npx -y ruflo@latest memory search -q "Mirai onboarding /me design tokens bubble visual UI" -n patterns -l 10 --smart --format json
npx -y ruflo@latest memory store -k "mirai/onboarding-ui-refresh/2026-05-07" -n patterns --value "<concise final pattern>" --tags "mirai,onboarding,ui,higgsfield" --upsert
```

Token-saving note:

- Report only relevant memory hits and exact keys. Do not paste full memories unless they contain direct implementation constraints.

### onboarding-assets-coder

Ownership:

- Baseline screenshots.
- Higgsfield inspiration generation.
- Bubble image generation.
- Image review artifact.

Required setup:

```bash
higgsfield account status
higgsfield model get gpt_image_2 --json > docs/superpowers/artifacts/onboarding-ui-refresh/gpt_image_2-schema.json
```

If `gpt_image_2` does not accept screenshot image inputs, stop and report. Do not switch models without approval.

Screenshot inputs:

```bash
npm run dev
browser-use open http://localhost:5173/onboarding
browser-use screenshot docs/superpowers/artifacts/onboarding-ui-refresh/current-onboarding-mobile.png
browser-use screenshot docs/superpowers/artifacts/onboarding-ui-refresh/current-onboarding-desktop.png
browser-use open http://localhost:5173/me
browser-use screenshot docs/superpowers/artifacts/onboarding-ui-refresh/current-me-mobile.png
browser-use screenshot docs/superpowers/artifacts/onboarding-ui-refresh/current-me-desktop.png
browser-use open http://localhost:5173/
browser-use screenshot docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-mobile.png
browser-use screenshot docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-desktop.png
```

Use the browser agent's viewport controls, or browser devtools, to capture mobile and desktop sizes.

Generate UI inspiration:

```bash
higgsfield generate create gpt_image_2 \
  --prompt "Mobile /onboarding screen for Mirai, a premium AI creator app. Use the supplied screenshots as design-token context. Strong /me-page inspired account hero, mint #4fd1a5 accent, charcoal #111318 premium panel, fashion beauty travel content creator aesthetic, source picker cards, image-filled inspiration bubbles with centered readable labels, polished iOS mobile product UI, no marketing hero, no fake browser chrome." \
  --image docs/superpowers/artifacts/onboarding-ui-refresh/current-onboarding-mobile.png \
  --image docs/superpowers/artifacts/onboarding-ui-refresh/current-me-mobile.png \
  --image docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-mobile.png \
  --aspect_ratio 9:16 --resolution 2k --wait --json \
  > docs/superpowers/artifacts/onboarding-ui-refresh/higgsfield-onboarding-mobile.json

higgsfield generate create gpt_image_2 \
  --prompt "Desktop /onboarding app screen for Mirai after login. Use supplied screenshots as design-token context. Preserve sidebar app shell, create a premium onboarding workspace with /me-page inspired status hero, source selection cards, starter creator grid, large visual bubble picker, fashion beauty vibes travel content niches, mint accent, charcoal premium surfaces, crisp Inter typography, real SaaS dashboard density, no fake browser chrome." \
  --image docs/superpowers/artifacts/onboarding-ui-refresh/current-onboarding-desktop.png \
  --image docs/superpowers/artifacts/onboarding-ui-refresh/current-me-desktop.png \
  --image docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-desktop.png \
  --aspect_ratio 16:10 --resolution 2k --wait --json \
  > docs/superpowers/artifacts/onboarding-ui-refresh/higgsfield-onboarding-desktop.json
```

Generate bubble backgrounds with no embedded text:

```bash
for niche in fashion beauty vibes travel content wellness streetwear editorial; do
  higgsfield generate create gpt_image_2 \
    --prompt "Square aesthetic background image for a circular inspiration bubble in a premium AI creator app. Niche: ${niche}. Fashion/beauty/vibes/travel/content creator taste, glossy editorial image, rich but clean composition, enough darker center contrast for white UI label overlay, no text, no logos, no watermark, consistent with mint #4fd1a5 and charcoal #111318 Mirai design context." \
    --image docs/superpowers/artifacts/onboarding-ui-refresh/current-me-mobile.png \
    --image docs/superpowers/artifacts/onboarding-ui-refresh/current-landing-mobile.png \
    --aspect_ratio 1:1 --resolution 2k --wait --json \
    > "docs/superpowers/artifacts/onboarding-ui-refresh/higgsfield-bubble-${niche}.json"
done
```

Save approved images to the file map paths. Do not reference unreviewed assets in source.

Image review criteria:

- Looks consistent with Mirai tokens and `/me` polish.
- No embedded text in bubble background assets.
- Bubble assets have enough center contrast for overlaid text.
- UI inspiration includes both mobile and desktop concepts.
- Visual direction clearly covers fashion, beauty, vibes, travel, and content.

Token-saving note:

- Give other agents file paths and a 1-2 sentence visual summary per approved image. Do not paste base64, JSON blobs, or long prompts into downstream prompts.

### onboarding-ui-coder

Ownership:

- `src/client/screens/OnboardingScreen.tsx`
- `src/client/screens/onboarding-visuals.ts`
- `src/client/onboarding.css`
- `src/client/main.tsx`

Implementation constraints:

- Keep API calls and state transitions intact.
- Keep `selectedBubbles.length >= 5` cap behavior.
- Add `aria-pressed` to bubble buttons.
- Keep source buttons disabled for bubbles until a clone exists.
- Prefer scoped classes prefixed with `onboarding-`.
- Import `./onboarding.css` in `src/client/main.tsx` after `./app-ui.css`.
- Do not edit `src/client/mobile.css` unless absolutely necessary; it is already large.
- Do not edit backend/schema.

Expected component changes:

- Hero becomes a stronger onboarding dashboard hero inspired by `/me`.
- Source selector becomes more descriptive and card-like while retaining four modes.
- Bubble mode renders image-backed circular buttons with centered text.
- Starter grid continues using existing portrait assets and overlay pattern.

Token-saving note:

- Read only file map files and the approved image review artifact. Do not inspect unrelated screens unless a selector collision is found.

### onboarding-tester

Ownership:

- `tests/client/onboarding-visuals.test.ts`
- `docs/superpowers/artifacts/onboarding-ui-refresh/browser-review.md`

Required checks:

```bash
npm run typecheck
npm test
npm run build
```

Browser review setup:

```bash
export MIRAI_TEST_EMAIL="<requester-provided-email>"
export MIRAI_TEST_PASSWORD="<requester-provided-password>"
npm run dev
```

Browser-use review:

```bash
browser-use open http://localhost:5173/login
browser-use state
```

Use browser-use or an equivalent browser agent to sign in with the environment-provided test credentials. Do not write credentials to source, docs, shell history snippets, or screenshots.

Review these routes after login:

- `/onboarding`
- `/me` for regression comparison only

Viewports:

- 390x844
- 430x932
- 768x1024
- 1440x900
- 1920x1080

Record in `browser-review.md`:

- Screenshot paths
- Pass/fail for layout overlap
- Pass/fail for readable bubble text
- Pass/fail for keyboard/focus states
- Pass/fail for source-mode switching
- Any console/network errors

Token-saving note:

- Report failures with screenshot path, route, viewport, and selector/class. Do not narrate every passing click.

### onboarding-reviewer

Ownership:

- Read-only diff review.

Review commands:

```bash
git diff --stat
git diff -- src/client/screens/OnboardingScreen.tsx src/client/screens/onboarding-visuals.ts src/client/onboarding.css src/client/main.tsx tests/client/onboarding-visuals.test.ts
```

Review focus:

- Onboarding flow regressions.
- API contract changes that were not approved.
- Accessibility regressions.
- Mobile/desktop overlap.
- Unreviewed images referenced by source.
- Secrets or credentials accidentally saved.
- CSS leakage into `/me` or other app screens.
- New files over 500 lines.

Token-saving note:

- Lead with findings only. If no findings, summarize residual risk and validation status.

## Validation Checklist

Automated:

- `npm run typecheck`
- `npm test`
- `npm run build`
- Secret scan using the runtime test email/password from environment variables via a temporary pattern file outside the repo.
- `wc -l src/client/screens/OnboardingScreen.tsx src/client/screens/onboarding-visuals.ts src/client/onboarding.css tests/client/onboarding-visuals.test.ts`

Manual/browser:

- Login succeeds using environment-supplied test credentials.
- `/onboarding` default mode is correct for a new or clone-less user.
- Source modes switch without errors.
- Upload form still accepts 5-15 images.
- Starter adoption still navigates to bubbles.
- Bubble selection caps at 5.
- Bubble selected state is visible in image-backed UI.
- Bubble text is centered and readable at all target viewports.
- Keyboard tab/focus states are visible.
- No horizontal overflow at 390px.
- Desktop does not look like a stretched mobile stack.
- `/me` still renders as before.

## Verification Matrix

| Area | Check | Owner | Pass condition |
| --- | --- | --- | --- |
| Design-token input | Current onboarding, `/me`, and landing screenshots captured | assets-coder | Screenshot files exist for mobile and desktop |
| Higgsfield model | `gpt_image_2` schema checked | assets-coder | Supports required screenshot input or task pauses |
| Image review | Generated images visually reviewed | assets-coder plus reviewer/Codex gate | Only approved images used in source |
| Bubble UI | Image-filled centered-text buttons | ui-coder | Readable, selectable, capped at 5 |
| API safety | Existing requests unchanged | reviewer | No server/API contract diff |
| Accessibility | Focus and `aria-pressed` | tester/reviewer | Keyboard and screen-reader state supported |
| Unit tests | Bubble visual helper tested | tester | Category and fallback tests pass |
| Build | Vite and Worker dry-run build | tester | `npm run build` passes |
| Browser | Mobile/tablet/desktop screenshots | tester | No overlap, overflow, or console errors |
| Security | No credentials committed | reviewer | Secret scan command has no hits |

## Verification-Quality Pass

Missing tests:

- Existing repo has no React DOM testing dependency. Plan avoids adding one and tests the pure bubble visual helper with Vitest.
- Visual behavior is covered by browser-use/manual screenshot review.

Ambiguous requirements:

- "After logging in" could imply all app screens, but this plan scopes implementation to `/onboarding` and uses `/me` only as reference.
- "Design token saved to AgentDB or docs" was checked; AgentDB search had no hits, so docs and screenshots are the source of truth.
- "Model capable of reviewing images" is treated as a blocking visual-review gate. If Ruflo DeepSeek cannot inspect images, Codex/GPT-5.5 or a browser agent must approve image files before coding.

Security risks:

- Test credentials must not be written into files. Use environment variables.
- Generated images must not include logos, watermarks, or embedded text that could imply endorsement.
- No user-uploaded private images are used for Higgsfield inspiration inputs.

API/provider risks:

- Ruflo MCP transport was unstable; CLI is primary.
- `agent spawn --provider openai --model deepseek-v4-pro` must be verified in the executor environment because CLI help examples default to other providers.
- Higgsfield `gpt_image_2` media input support must be verified with `higgsfield model get gpt_image_2 --json`.
- Browser-use may not be installed; if absent, install or use an equivalent browser agent, but keep the same viewport checklist.

Token-waste risks:

- Do not paste full plan into every agent prompt; reference this path.
- Do not paste screenshots, base64, Higgsfield JSON, or full source files into agent prompts.
- Keep each agent in its ownership lane.

Likely integration failures:

- CSS selector leakage because existing onboarding styles live in shared CSS.
- Text overflow inside circular bubbles at small widths.
- Generated bubble images too busy for centered text.
- `main.tsx` CSS import order wrong, causing overrides not to apply.
- Dirty worktree causing reviewer confusion; always review only declared paths.

Narrow prompt boundaries:

- assets-coder cannot edit `src`.
- ui-coder cannot generate assets or edit server.
- tester cannot alter implementation files unless explicitly coordinated.
- reviewer is read-only.

## Rollback Notes

No database migration is planned.

If rollback is needed:

```bash
git diff -- src/client/screens/OnboardingScreen.tsx src/client/screens/onboarding-visuals.ts src/client/onboarding.css src/client/main.tsx tests/client/onboarding-visuals.test.ts
```

Then selectively revert only the files owned by this plan. Do not run `git reset --hard` and do not revert unrelated dirty worktree changes.

Generated assets can be removed if they are not referenced:

```bash
rm -rf public/landing/ui-inspiration/onboarding
rm -rf public/landing/onboarding-bubbles
rm -rf docs/superpowers/artifacts/onboarding-ui-refresh
```

Only remove those directories if they were created by this execution and no source file references them.

## Completion Criteria

- User approved this plan before Ruflo execution.
- All task graph gates completed in order.
- Higgsfield images generated with `gpt_image_2` and reviewed before use.
- `/onboarding` improved on mobile and desktop.
- Inspiration bubbles contain aesthetic images with centered readable text.
- Existing onboarding behavior preserved.
- Typecheck, tests, and build pass.
- Browser-use/manual review passes target viewports.
- Reviewer reports no blocking findings.
- Memory entry stored with concise final pattern.
