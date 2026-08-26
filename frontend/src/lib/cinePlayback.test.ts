import { describe, expect, it, vi } from "vitest";
import {
	buildCineLookahead,
	canRunCinePlayback,
	cineFrameIntervalMs,
	nextCineStep,
	runRenderPacedCine,
	waitForAbortableResult,
	waitForCineDeadline,
} from "./cinePlayback";

describe("cine playback policy", () => {
	it("allows playback only in the display pipeline with multiple pixel frames", () => {
		expect(canRunCinePlayback("cine", true, 2)).toBe(true);
		expect(canRunCinePlayback("diagnostic_wl", true, 2)).toBe(false);
		expect(canRunCinePlayback("cine", false, 2)).toBe(false);
		expect(canRunCinePlayback("cine", true, 1)).toBe(false);
	});

	it("wraps loop playback in both directions", () => {
		expect(nextCineStep(4, 5, "loop", 1)).toEqual({ frame: 0, direction: 1 });
		expect(nextCineStep(0, 5, "loop", -1)).toEqual({ frame: 4, direction: -1 });
	});

	it("reverses sweep playback without holding the endpoint", () => {
		expect(nextCineStep(4, 5, "sweep", 1)).toEqual({ frame: 3, direction: -1 });
		expect(nextCineStep(0, 5, "sweep", -1)).toEqual({ frame: 1, direction: 1 });
	});

	it("builds circular loop lookahead across the stack boundary", () => {
		expect(buildCineLookahead(3, 5, "loop", 1, 4)).toEqual([4, 0, 1, 2]);
	});

	it("builds direction-aware sweep lookahead", () => {
		expect(buildCineLookahead(3, 5, "sweep", 1, 4)).toEqual([4, 2, 1, 0]);
		expect(buildCineLookahead(4, 5, "sweep", 1, 4)).toEqual([3, 2, 1, 0]);
	});

	it("treats configured FPS as an interval ceiling", () => {
		expect(cineFrameIntervalMs(10)).toBe(100);
		expect(cineFrameIntervalMs(0)).toBe(1000);
	});

	it("releases abort listeners and subscriptions after normal completion", async () => {
		const ctrl = new AbortController();
		const addListener = vi.spyOn(ctrl.signal, "addEventListener");
		const removeListener = vi.spyOn(ctrl.signal, "removeEventListener");
		const cleanup = vi.fn();
		const subscription: { settle?: (value: boolean) => void } = {};
		const result = waitForAbortableResult(ctrl.signal, (finish) => {
			subscription.settle = finish;
			return cleanup;
		});

		subscription.settle?.(true);

		await expect(result).resolves.toBe(true);
		expect(cleanup).toHaveBeenCalledOnce();
		expect(removeListener).toHaveBeenCalledWith("abort", addListener.mock.calls[0][1]);
	});

	it("cleans up an abortable wait when playback stops", async () => {
		const ctrl = new AbortController();
		const cleanup = vi.fn();
		const result = waitForAbortableResult(ctrl.signal, () => cleanup);

		ctrl.abort();

		await expect(result).resolves.toBe(false);
		expect(cleanup).toHaveBeenCalledOnce();
	});

	it("releases the cine deadline listener after its timer fires", async () => {
		vi.useFakeTimers();
		const ctrl = new AbortController();
		const removeListener = vi.spyOn(ctrl.signal, "removeEventListener");
		const result = waitForCineDeadline(25, ctrl.signal);

		await vi.advanceTimersByTimeAsync(25);

		await expect(result).resolves.toBe(true);
		expect(removeListener).toHaveBeenCalledOnce();
		vi.useRealTimers();
	});

	it("waits for slow frame preparation instead of issuing catch-up frames", async () => {
		const ctrl = new AbortController();
		const presentedAt: number[] = [];
		await runRenderPacedCine({
			initialFrame: 0,
			totalFrames: 4,
			mode: "loop",
			direction: 1,
			fps: 100,
			signal: ctrl.signal,
			now: () => performance.now(),
			waitForDelay: (delay, signal) => new Promise((resolve) => {
				const timer = setTimeout(() => resolve(true), Math.max(0, delay));
				signal.addEventListener("abort", () => {
					clearTimeout(timer);
					resolve(false);
				}, { once: true });
			}),
			prepareFrame: () => new Promise((resolve) => setTimeout(resolve, 25)),
			presentFrame: async () => {
				presentedAt.push(performance.now());
				if (presentedAt.length === 3) ctrl.abort();
				return true;
			},
		});

		expect(presentedAt).toHaveLength(3);
		expect(presentedAt[1] - presentedAt[0]).toBeGreaterThanOrEqual(20);
		expect(presentedAt[2] - presentedAt[1]).toBeGreaterThanOrEqual(20);
	});
});
