import { describe, expect, it, vi } from "vitest";
import { trackForegroundRequest } from "./requestIndicator";

describe("foreground request indicator", () => {
	it("stays hidden when a frame is already cached", () => {
		const setPending = vi.fn();
		trackForegroundRequest(undefined, () => true, setPending);
		expect(setPending).toHaveBeenCalledOnce();
		expect(setPending).toHaveBeenCalledWith(false);
	});

	it("covers the network request but ignores later local work", async () => {
		let resolveRequest: (() => void) | undefined;
		const request = new Promise<void>((resolve) => {
			resolveRequest = resolve;
		});
		const setPending = vi.fn();

		trackForegroundRequest(request, () => true, setPending);
		expect(setPending).toHaveBeenLastCalledWith(true);

		resolveRequest?.();
		await request;
		await Promise.resolve();
		expect(setPending).toHaveBeenLastCalledWith(false);
	});

	it("does not let a stale request hide a newer indicator", async () => {
		let current = true;
		let resolveRequest: (() => void) | undefined;
		const request = new Promise<void>((resolve) => {
			resolveRequest = resolve;
		});
		const setPending = vi.fn();

		trackForegroundRequest(request, () => current, setPending);
		current = false;
		resolveRequest?.();
		await request;
		await Promise.resolve();

		expect(setPending).toHaveBeenCalledTimes(1);
		expect(setPending).toHaveBeenCalledWith(true);
	});
});
