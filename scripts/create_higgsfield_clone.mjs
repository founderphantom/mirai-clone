#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { basename, extname, join, resolve } from "node:path";

const IMAGE_EXTENSIONS = new Set([".jpg", ".jpeg", ".png", ".webp"]);

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.name) {
    printHelp();
    process.exit(args.help ? 0 : 1);
  }

  const images = collectImages(args);
  const referenceCount = images.length + args.uploadedImageIds.length;
  if (referenceCount < 5 || referenceCount > 20) {
    fail(`Higgsfield Soul-ID requires 5-20 reference images. Found ${referenceCount}.`);
  }

  const uploadedIds = [...args.uploadedImageIds];
  for (const image of images) {
    const upload = runJson(["upload", "create", image, "--json"]);
    const id = extractId(upload);
    if (!id) fail(`Could not find uploaded media id in CLI response for ${image}.`);
    uploadedIds.push(id);
    log(`uploaded ${basename(image)} -> ${id}`);
  }

  const createArgs = ["soul-id", "create", "--name", args.name, args.modelFlag, "--json"];
  for (const id of uploadedIds) createArgs.push("--image", id);

  const created = runJson(createArgs);
  const soulId = extractId(created);
  if (!soulId) fail("Could not find Soul-ID in Higgsfield create response.");
  log(`created Soul-ID ${soulId}`);

  let finalState = created;
  if (args.wait) {
    finalState = runJson([
      "soul-id",
      "wait",
      soulId,
      "--timeout",
      args.timeout,
      "--interval",
      args.interval,
      "--json",
      "--quiet"
    ]);
  }

  const result = {
    status: "success",
    name: args.name,
    soulId,
    uploadedImageIds: uploadedIds,
    providerConfig: {
      customReferenceId: soulId,
      styleId: args.styleId || undefined,
      styleStrength: args.styleStrength
    },
    higgsfield: {
      create: created,
      final: finalState
    }
  };

  if (args.output) {
    const outputPath = resolve(args.output);
    mkdirSync(resolve(outputPath, ".."), { recursive: true });
    writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`);
    log(`wrote ${outputPath}`);
  }

  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}

function parseArgs(argv) {
  const args = {
    help: false,
    name: "",
    imagePaths: [],
    imageDir: "",
    uploadedImageIds: [],
    modelFlag: "--soul-2",
    wait: false,
    timeout: "45m",
    interval: "15s",
    output: "",
    styleId: "",
    styleStrength: 1
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) fail(`Missing value for ${arg}.`);
      return argv[i];
    };

    if (arg === "-h" || arg === "--help") args.help = true;
    else if (arg === "--name") args.name = next();
    else if (arg === "--image") args.imagePaths.push(next());
    else if (arg === "--image-dir") args.imageDir = next();
    else if (arg === "--uploaded-image") args.uploadedImageIds.push(next());
    else if (arg === "--soul-2") args.modelFlag = "--soul-2";
    else if (arg === "--soul-cinematic") args.modelFlag = "--soul-cinematic";
    else if (arg === "--wait") args.wait = true;
    else if (arg === "--timeout") args.timeout = next();
    else if (arg === "--interval") args.interval = next();
    else if (arg === "--output") args.output = next();
    else if (arg === "--style-id") args.styleId = next();
    else if (arg === "--style-strength") args.styleStrength = Number(next());
    else fail(`Unknown argument: ${arg}`);
  }

  if (!Number.isFinite(args.styleStrength)) fail("--style-strength must be a number.");
  return args;
}

function collectImages(args) {
  const paths = args.imagePaths.map((item) => resolve(item));

  if (args.imageDir) {
    const dir = resolve(args.imageDir);
    if (!existsSync(dir)) fail(`Image directory does not exist: ${dir}`);
    for (const entry of readdirSync(dir)) {
      const fullPath = join(dir, entry);
      if (IMAGE_EXTENSIONS.has(extname(entry).toLowerCase())) paths.push(fullPath);
    }
  }

  for (const image of paths) {
    if (!existsSync(image)) fail(`Image does not exist: ${image}`);
  }
  return [...new Set(paths)];
}

function runJson(args) {
  const command = process.platform === "win32" ? "higgsfield.cmd" : "higgsfield";
  const result = spawnSync(command, args, {
    encoding: "utf8",
    shell: false
  });

  if (result.status !== 0) {
    fail(
      `higgsfield ${args.join(" ")} failed.\n${result.stderr || result.stdout || "No CLI output."}`
    );
  }

  try {
    return JSON.parse(result.stdout);
  } catch {
    fail(`Could not parse Higgsfield JSON output.\n${result.stdout}`);
  }
}

function extractId(value) {
  if (!value || typeof value !== "object") return "";
  if (typeof value.id === "string") return value.id;
  if (typeof value.soul_id === "string") return value.soul_id;
  if (typeof value.soulId === "string") return value.soulId;
  if (typeof value.media_id === "string") return value.media_id;
  if (typeof value.mediaId === "string") return value.mediaId;
  if (typeof value.upload_id === "string") return value.upload_id;
  if (typeof value.uploadId === "string") return value.uploadId;
  if (Array.isArray(value.items) && value.items.length > 0) return extractId(value.items[0]);
  if (value.data) return extractId(value.data);
  if (value.result) return extractId(value.result);
  return "";
}

function printHelp() {
  process.stdout.write(`Create a Higgsfield Soul-ID clone and emit Mirai providerConfig.

Usage:
  npm run higgsfield:clone:create -- --name "Clone Name" --image-dir ./references --wait
  npm run higgsfield:clone:create -- --name "Clone Name" --image ./1.jpg --image ./2.jpg --image ./3.jpg --image ./4.jpg --image ./5.jpg --wait

Options:
  --name <name>              Higgsfield Soul-ID name. Required.
  --image <path>             Reference image path. Repeatable.
  --image-dir <path>         Directory of jpg/png/webp reference images.
  --uploaded-image <id>      Existing Higgsfield upload id. Repeatable.
  --soul-2                   Train Soul 2.0 model. Default.
  --soul-cinematic           Train Soul Cinematic model.
  --wait                     Wait until training finishes.
  --timeout <duration>       Wait timeout. Default: 45m.
  --interval <duration>      Wait poll interval. Default: 15s.
  --style-id <id>            Optional default style id for Mirai providerConfig.
  --style-strength <number>  Optional style strength. Default: 1.
  --output <path>            Write result JSON to a file.
`);
}

function log(message) {
  process.stderr.write(`[higgsfield-clone] ${message}\n`);
}

function fail(message) {
  process.stderr.write(`[higgsfield-clone] ${message}\n`);
  process.exit(1);
}

main();
