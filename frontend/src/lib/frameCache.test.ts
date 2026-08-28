import { describe, expect, it, vi } from "vitest";
import {
	ByteBudgetLruCache,
	createDisplayFrameCache,
	decodedBitmapBytes,
	type BitmapResource,
} from "./frameCache";

function bitmap(width: number, height: number): BitmapResource {
	return { width, height, close: vi.fn() };
}

describe("ByteBudgetLruCache", () => {
	it("retains visited frames from different source files within one budget", () => {
		const cache = new ByteBudgetLruCache<string, { bytes: number }>({
			maxBytes: 12,
			sizeOf: (entry) => entry.bytes,
		});
		cache.set("7:0", { bytes: 4 });
		cache.set("9:0", { bytes: 4 });
		cache.set("9:1", { bytes: 4 });

		expect([...cache.keys()]).toEqual(["7:0", "9:0", "9:1"]);
		expect(cache.get("7:0")).toEqual({ bytes: 4 });
		expect([...cache.keys()]).toEqual(["9:0", "9:1", "7:0"]);
	});

	it("evicts the least-recently-used entry within its byte budget", () => {
		const disposed: string[] = [];
		const cache = new ByteBudgetLruCache<string, { id: string; bytes: number }>({
			maxBytes: 10,
			sizeOf: (entry) => entry.bytes,
			dispose: (entry) => disposed.push(entry.id),
		});
		cache.set("a", { id: "a", bytes: 4 });
		cache.set("b", { id: "b", bytes: 4 });
		cache.get("a");

		expect(cache.set("c", { id: "c", bytes: 4 })).toBe(true);
		expect([...cache.keys()]).toEqual(["a", "c"]);
		expect(disposed).toEqual(["b"]);
		expect(cache.bytes).toBe(8);
	});

	it("rejects oversized entries without replacing a cached value", () => {
		const dispose = vi.fn();
		const cache = new ByteBudgetLruCache<string, { bytes: number }>({
			maxBytes: 4,
			sizeOf: (entry) => entry.bytes,
			dispose,
		});
		const retained = { bytes: 4 };
		cache.set("frame", retained);
		const oversized = { bytes: 5 };

		expect(cache.set("frame", oversized)).toBe(false);
		expect(cache.peek("frame")).toBe(retained);
		expect(dispose).toHaveBeenCalledWith(oversized);
	});
});

describe("display frame cache accounting", () => {
	it("accounts RGBA bitmap memory in addition to retained blob bytes", () => {
		const decoded = bitmap(4, 3);
		expect(decodedBitmapBytes(decoded)).toBe(48);

		const cache = createDisplayFrameCache<BitmapResource>(58);
		expect(cache.set("one", { blob: new Blob([new Uint8Array(10)]), bitmap: decoded })).toBe(true);
		expect(cache.bytes).toBe(58);
	});

	it("closes decoded bitmaps when pressure evicts them", () => {
		const first = bitmap(4, 2);
		const second = bitmap(4, 2);
		const cache = createDisplayFrameCache<BitmapResource>(50);

		cache.set("first", { blob: new Blob([new Uint8Array(10)]), bitmap: first });
		cache.set("second", { blob: new Blob([new Uint8Array(10)]), bitmap: second });

		expect(cache.has("first")).toBe(false);
		expect(first.close).toHaveBeenCalledOnce();
		expect(cache.has("second")).toBe(true);
		expect(second.close).not.toHaveBeenCalled();
	});

	it("closes every retained bitmap when cleared after a stress fill", () => {
		const frames = Array.from({ length: 200 }, () => bitmap(2, 2));
		const cache = createDisplayFrameCache<BitmapResource>(20 * 25);

		for (let index = 0; index < frames.length; index += 1) {
			cache.set(String(index), {
				blob: new Blob([new Uint8Array(4)]),
				bitmap: frames[index],
			});
			expect(cache.bytes).toBeLessThanOrEqual(cache.maxBytes);
		}
		cache.clear();

		expect(cache.bytes).toBe(0);
		expect(cache.size).toBe(0);
		expect(frames.every((frame) => vi.mocked(frame.close).mock.calls.length === 1)).toBe(true);
	});
});
