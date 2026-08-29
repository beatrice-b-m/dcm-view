import { describe, expect, it } from "vitest";
import { wsiMinimapGeometry } from "./wsiMinimap";

describe("wsiMinimapGeometry", () => {
	it("positions a selected tile without allocating for the matrix", () => {
		const result = wsiMinimapGeometry(
			{ rows: 1_000_000, columns: 2_000_000 },
			{ x: 1_500_000, y: 500_000, width: 512, height: 256 },
		);
		expect(result?.viewWidth).toBe(160);
		expect(result?.viewHeight).toBe(80);
		expect(result?.tile.x).toBeCloseTo(120);
		expect(result?.tile.y).toBeCloseTo(40);
		expect(result?.tile.width).toBeCloseTo(0.04096);
		expect(result?.tile.height).toBeCloseTo(0.02048);
	});

	it("rejects missing and out-of-matrix placement", () => {
		expect(wsiMinimapGeometry(null, null)).toBeNull();
		expect(
			wsiMinimapGeometry(
				{ rows: 100, columns: 100 },
				{ x: 100, y: 0, width: 10, height: 10 },
			),
		).toBeNull();
	});
});
