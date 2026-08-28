import type { FrameRefSummary, SeriesStackSummary, SeriesSummary } from "../api";

export interface LocatedSeriesStack {
	series: SeriesSummary;
	stack: SeriesStackSummary;
}

export function findSeriesStackForFile(
	series: readonly SeriesSummary[],
	fileIndex: number,
): LocatedSeriesStack | null {
	for (const item of series) {
		for (const stack of item.stacks) {
			if (stack.frames.some((frame) => frame.file_index === fileIndex)) {
				return { series: item, stack };
			}
		}
	}
	return null;
}

export function framePosition(
	stack: SeriesStackSummary,
	fileIndex: number,
	frameIndex: number,
): number | null {
	const exact = stack.frames.findIndex(
		(frame) => frame.file_index === fileIndex && frame.frame_index === frameIndex,
	);
	if (exact >= 0) return exact;
	const source = stack.frames.findIndex((frame) => frame.file_index === fileIndex);
	return source >= 0 ? source : null;
}

export function frameAtPosition(
	stack: SeriesStackSummary | null,
	position: number,
): FrameRefSummary | null {
	if (!stack || stack.frames.length === 0) return null;
	const bounded = Math.max(0, Math.min(stack.frames.length - 1, position));
	return stack.frames[bounded] ?? null;
}

export function navigationTabId(
	series: readonly SeriesSummary[],
	fileIndex: number,
): string {
	return findSeriesStackForFile(series, fileIndex)?.stack.id ?? `file:${fileIndex}`;
}
