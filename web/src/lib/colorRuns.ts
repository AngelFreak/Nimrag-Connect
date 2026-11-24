export interface ColorRun {
  color: string;
  /** First and last point (inclusive) of the stretch; a run ends on the point the next one starts on, so there is no gap. */
  from: number;
  to: number;
}

/** Consecutive points of one colour as runs (a coloured route is drawn as one line per run). */
export function colorRuns(colors: readonly string[]): ColorRun[] {
  const runs: ColorRun[] = [];
  colors.forEach((color, i) => {
    const last = runs[runs.length - 1];
    if (last && last.color === color) {
      last.to = i;
      return;
    }
    if (last) last.to = i;
    runs.push({ color, from: i, to: i });
  });
  return runs.filter((r) => r.to > r.from);
}
