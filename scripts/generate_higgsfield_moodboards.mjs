#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const ENDPOINT = "https://fnf.higgsfield.ai/soul-v2/presets?size=100";
const EXPECTED_COUNT = 32;
const SOURCE_DIR = "docs/images/higgsfield-moodboards/source";
const OUTPUT_DIR = "public/landing/moodboards";
const OUTPUT_EXT = "webp";

const args = parseArgs(process.argv.slice(2));

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});

async function main() {
  const presets = await fetchMoodboards();
  const selected = filterSelection(presets);

  if (!args.allowCountMismatch && selected.length !== EXPECTED_COUNT) {
    throw new Error(`Expected ${EXPECTED_COUNT} moodboards after filtering General, received ${selected.length}.`);
  }

  await mkdir(SOURCE_DIR, { recursive: true });
  if (!args.downloadOnly) await mkdir(OUTPUT_DIR, { recursive: true });

  for (const preset of selected) {
    const slug = slugify(preset.name);
    const sourceUrl = preset.media?.url || preset.medias?.[0]?.url;
    if (!sourceUrl) throw new Error(`Preset ${preset.name} has no source media URL.`);

    const sourcePath = join(SOURCE_DIR, `${slug}${sourceExtension(sourceUrl)}`);
    const outputPath = join(OUTPUT_DIR, `${slug}.${OUTPUT_EXT}`);

    if (args.dryRun) {
      console.log(`${preset.name} -> ${sourcePath} -> ${outputPath}`);
      continue;
    }

    if (args.overwrite || !existsSync(sourcePath)) {
      await downloadFile(sourceUrl, sourcePath);
    }

    if (args.downloadOnly) continue;
    if (!args.overwrite && existsSync(outputPath)) {
      console.log(`Skipping existing ${outputPath}`);
      continue;
    }

    const generatedUrl = runGptImage2(preset.name, sourcePath);
    await downloadFile(generatedUrl, outputPath);
    console.log(`Wrote ${outputPath}`);
  }
}

async function fetchMoodboards() {
  const response = await fetch(ENDPOINT);
  if (!response.ok) {
    throw new Error(`Failed to fetch Higgsfield presets: ${response.status} ${response.statusText}`);
  }
  const payload = await response.json();
  if (!Array.isArray(payload.items)) {
    throw new Error("Higgsfield preset response did not include an items array.");
  }
  return payload.items.filter((item) => item?.name && item.name !== "General");
}

function filterSelection(presets) {
  const selected = args.only.length === 0
    ? presets
    : presets.filter((preset) => {
        const slug = slugify(preset.name);
        return args.only.includes(slug) || args.only.includes(preset.name.toLowerCase());
      });
  return args.limit === null ? selected : selected.slice(0, args.limit);
}

function runGptImage2(name, sourcePath) {
  const prompt = [
    `Create a polished square frontend moodboard thumbnail for "${name}".`,
    "Use the attached image only as a style reference.",
    "High-fidelity editorial creator aesthetic, no text, no logos, no watermark."
  ].join(" ");

  const result = spawnSync(
    "higgsfield",
    [
      "generate",
      "create",
      "gpt_image_2",
      "--prompt",
      prompt,
      "--image",
      sourcePath,
      "--aspect_ratio",
      "1:1",
      "--resolution",
      "2k",
      "--quality",
      "high",
      "--wait",
      "--wait-timeout",
      "20m",
      "--json"
    ],
    { encoding: "utf8" }
  );

  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`Higgsfield generation failed for ${name}:\n${result.stderr || result.stdout}`);
  }

  const url = extractImageUrl(result.stdout);
  if (!url) throw new Error(`Could not find a generated image URL for ${name}.`);
  return url;
}

async function downloadFile(url, path) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Failed to download ${url}: ${response.status} ${response.statusText}`);
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  await writeFile(path, bytes);
}

function extractImageUrl(stdout) {
  try {
    const parsed = JSON.parse(stdout);
    const urls = [];
    collectUrls(parsed, urls);
    const imageUrl = urls.find((url) => /\.(png|jpe?g|webp)(\?|$)/i.test(url));
    if (imageUrl) return imageUrl;
    if (urls.length > 0) return urls[0];
  } catch {
    // Fall through to regex extraction for non-JSON CLI output.
  }

  return stdout.match(/https?:\/\/\S+/)?.[0]?.replace(/[),\]]+$/, "") ?? null;
}

function collectUrls(value, urls) {
  if (typeof value === "string" && value.startsWith("http")) {
    urls.push(value);
    return;
  }
  if (Array.isArray(value)) {
    for (const item of value) collectUrls(item, urls);
    return;
  }
  if (value && typeof value === "object") {
    for (const item of Object.values(value)) collectUrls(item, urls);
  }
}

function sourceExtension(url) {
  const pathname = new URL(url).pathname;
  const match = pathname.match(/\.(png|jpe?g|webp)$/i);
  return match ? match[0].toLowerCase() : ".webp";
}

function slugify(value) {
  return value
    .trim()
    .toLowerCase()
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function parseArgs(argv) {
  const parsed = {
    allowCountMismatch: false,
    downloadOnly: false,
    dryRun: false,
    limit: null,
    only: [],
    overwrite: false
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--allow-count-mismatch") parsed.allowCountMismatch = true;
    else if (arg === "--download-only") parsed.downloadOnly = true;
    else if (arg === "--dry-run") parsed.dryRun = true;
    else if (arg === "--overwrite") parsed.overwrite = true;
    else if (arg === "--limit") parsed.limit = Number(argv[++index]);
    else if (arg === "--only") parsed.only.push(String(argv[++index] || "").toLowerCase());
    else throw new Error(`Unknown argument: ${arg}`);
  }

  if (parsed.limit !== null && (!Number.isInteger(parsed.limit) || parsed.limit < 1)) {
    throw new Error("--limit must be a positive integer.");
  }

  return parsed;
}
