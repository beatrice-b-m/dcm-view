import { describe, expect, it } from "vitest";
import {
	displayFrameCacheKey,
	displayFrameWindowCacheKey,
	parseRawFrameMetadata,
} from "./api";
import { RAW_FRAME_HEADERS } from "./generated/api-types";

function completeRawHeaders(): Headers {
	return new Headers({
		[RAW_FRAME_HEADERS.rows]: "512",
		[RAW_FRAME_HEADERS.columns]: "256",
		[RAW_FRAME_HEADERS.bitsAllocated]: "16",
		[RAW_FRAME_HEADERS.pixelRepresentation]: "1",
		[RAW_FRAME_HEADERS.samplesPerPixel]: "1",
		[RAW_FRAME_HEADERS.photometricInterpretation]: "MONOCHROME1",
		[RAW_FRAME_HEADERS.rescaleSlope]: "2.5",
		[RAW_FRAME_HEADERS.rescaleIntercept]: "-1024",
		[RAW_FRAME_HEADERS.defaultWc]: "40",
		[RAW_FRAME_HEADERS.defaultWw]: "80",
	});
}

describe("parseRawFrameMetadata", () => {
	it("maps the generated raw-frame header contract", () => {
		expect(parseRawFrameMetadata(completeRawHeaders())).toEqual({
			rows: 512,
			columns: 256,
			bitsAllocated: 16,
			pixelRepresentation: 1,
			samplesPerPixel: 1,
			photometricInterpretation: "MONOCHROME1",
			rescaleSlope: 2.5,
			rescaleIntercept: -1024,
			defaultWc: 40,
			defaultWw: 80,
		});
	});

	it("represents absent optional window headers as null", () => {
		const headers = completeRawHeaders();
		headers.delete(RAW_FRAME_HEADERS.defaultWc);
		headers.delete(RAW_FRAME_HEADERS.defaultWw);

		expect(parseRawFrameMetadata(headers)).toMatchObject({
			defaultWc: null,
			defaultWw: null,
		});
	});

	it("rejects missing or partially numeric required headers", () => {
		const missing = completeRawHeaders();
		missing.delete(RAW_FRAME_HEADERS.rows);
		expect(() => parseRawFrameMetadata(missing)).toThrow(
			`raw frame response missing required header ${RAW_FRAME_HEADERS.rows}`,
		);

		const malformed = completeRawHeaders();
		malformed.set(RAW_FRAME_HEADERS.columns, "256px");
		expect(() => parseRawFrameMetadata(malformed)).toThrow(
			`raw frame response has invalid integer header ${RAW_FRAME_HEADERS.columns}`,
		);
	});
});

describe("display frame cache keys", () => {
	it("canonicalizes absent window values and includes the window mode", () => {
		expect(displayFrameWindowCacheKey()).toBe("default:none:none");
		expect(displayFrameWindowCacheKey({ wc: null, ww: undefined })).toBe(
			"default:none:none",
		);
		expect(displayFrameCacheKey(2, 7, { windowMode: "full_dynamic" })).toBe(
			"2:7:full_dynamic:none:none",
		);
		expect(
			displayFrameCacheKey(2, 7, {
				windowMode: "full_dynamic",
				wc: 40,
				ww: 80,
			}),
		).toBe("2:7:full_dynamic:none:none");
	});

	it("does not conflate distinct window parameters sent to the backend", () => {
		const first = displayFrameCacheKey(0, 0, { wc: 1.00001, ww: 2.00001 });
		const second = displayFrameCacheKey(0, 0, { wc: 1.00002, ww: 2.00002 });

		expect(first).not.toBe(second);
	});
});
