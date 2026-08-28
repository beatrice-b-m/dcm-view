import { describe, expect, it } from "vitest";
import {
	effectivePixelAspectRatio,
	fitImageToViewportHeight,
	imageDisplayGeometry,
} from "./imageGeometry";

describe("physical image geometry", () => {
	it("uses square pixels when physical aspect is absent or invalid", () => {
		expect(effectivePixelAspectRatio(null)).toBe(1);
		expect(effectivePixelAspectRatio(0)).toBe(1);
		expect(effectivePixelAspectRatio(Number.POSITIVE_INFINITY)).toBe(1);
		expect(imageDisplayGeometry(4, 6, null)).toMatchObject({ width: 6, height: 4 });
	});

	it("scales row extent while preserving the pixel coordinate grid", () => {
		expect(imageDisplayGeometry(4, 6, 2)).toEqual({
			width: 6,
			height: 8,
			centerX: 3,
			centerY: 4,
			pixelAspectRatio: 2,
		});
	});

	it("fits and centers using physical rather than stored pixel dimensions", () => {
		const geometry = imageDisplayGeometry(4, 6, 2);
		expect(fitImageToViewportHeight(geometry, 100, 80, 0.05)).toEqual({
			scale: 10,
			tx: 20,
			ty: 0,
		});
	});
});
