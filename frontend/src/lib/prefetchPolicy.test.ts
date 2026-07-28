import { describe, expect, it } from "vitest";
import {
	buildDirectionalFrameOrder,
	planDisplayPrefetchTargets,
	shouldPrefetchWholeDisplayStack,
} from "./prefetchPolicy";

describe("display prefetch policy", () => {
	it("orders nearby frames in the current direction without duplicates", () => {
		expect(buildDirectionalFrameOrder(3, 7, 3, 1)).toEqual([4, 2, 5, 1, 6, 0]);
		expect(buildDirectionalFrameOrder(3, 7, 2, -1)).toEqual([2, 4, 1, 5]);
	});

	it("prefetches an affordable stack but caps a large stack to the near radius", () => {
		expect(shouldPrefetchWholeDisplayStack(8, 10, 80)).toBe(true);
		expect(
			planDisplayPrefetchTargets({
				startFrame: 3,
				totalFrames: 8,
				direction: 1,
				currentBlobBytes: 10,
				fullStackBudgetBytes: 80,
				nearDistance: 2,
			}),
		).toHaveLength(7);

		expect(
			planDisplayPrefetchTargets({
				startFrame: 50,
				totalFrames: 10_000,
				direction: 1,
				currentBlobBytes: 1_000_000,
				fullStackBudgetBytes: 80_000_000,
				nearDistance: 4,
			}),
		).toEqual([51, 49, 52, 48, 53, 47, 54, 46]);
	});

	it("uses bounded forward-only lookahead during cine playback", () => {
		expect(
			planDisplayPrefetchTargets({
				startFrame: 7,
				totalFrames: 10,
				direction: 1,
				currentBlobBytes: 1,
				fullStackBudgetBytes: 100,
				nearDistance: 4,
				forwardOnly: true,
				lookaheadFrames: 16,
			}),
		).toEqual([8, 9]);
	});
});
