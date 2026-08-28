import { describe, expect, it, vi } from "vitest";
import { SharedRequestRegistry } from "./sharedRequestRegistry";

describe("SharedRequestRegistry", () => {
	it("shares one in-flight request between consumers", async () => {
		let resolve!: (value: string) => void;
		const load = vi.fn(() => new Promise<string>((done) => { resolve = done; }));
		const registry = new SharedRequestRegistry<string, string>();

		const prefetch = registry.request("4:7", load);
		const foreground = registry.request("4:7", load);

		expect(foreground).toBe(prefetch);
		expect(registry.get("4:7")).toBe(prefetch);
		expect(load).toHaveBeenCalledOnce();

		resolve("frame");
		await expect(foreground).resolves.toBe("frame");
		expect(registry.get("4:7")).toBeUndefined();
	});

	it("aborts owned requests only when the registry scope is cleared", async () => {
		let requestSignal!: AbortSignal;
		const registry = new SharedRequestRegistry<string, string>();
		const request = registry.request("2:3", (signal) => {
			requestSignal = signal;
			return new Promise<string>((_resolve, reject) => {
				signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
			});
		});

		expect(requestSignal.aborted).toBe(false);
		registry.abortAll();
		expect(requestSignal.aborted).toBe(true);
		await expect(request).rejects.toMatchObject({ name: "AbortError" });
		expect(registry.get("2:3")).toBeUndefined();
	});
});
