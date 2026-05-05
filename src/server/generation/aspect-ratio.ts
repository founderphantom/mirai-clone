const RATIOS = [
  ["1:1", 1],
  ["2:3", 2 / 3],
  ["3:2", 3 / 2],
  ["3:4", 3 / 4],
  ["4:3", 4 / 3],
  ["9:16", 9 / 16],
  ["16:9", 16 / 9]
] as const;

export function closestAspectRatio(width?: number | null, height?: number | null): string {
  if (!width || !height || width <= 0 || height <= 0) return "3:4";
  const actual = width / height;
  return RATIOS.reduce(
    (best, current) => {
      const diff = Math.abs(actual - current[1]);
      return diff < best.diff ? { label: current[0], diff } : best;
    },
    { label: "1:1", diff: Number.POSITIVE_INFINITY }
  ).label;
}

export function dimensionsForQuality(quality: string): { width: number; height: number } {
  if (quality === "2K") return { width: 2048, height: 2732 };
  return { width: 1536, height: 2048 };
}
