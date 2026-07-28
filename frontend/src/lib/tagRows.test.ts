import { describe, expect, it } from "vitest";
import type { TagNode } from "../api";
import {
	flattenTagRows,
	tagValueDisplay,
	tagValueToCopyText,
} from "./tagRows";

const tags: TagNode[] = [
	{
		tag: "(0008,0016)",
		keyword: "SOPClassUID",
		vr: "UI",
		value: { type: "string", value: "1.2.3" },
	},
	{
		tag: "(0008,1115)",
		keyword: "ReferencedSeriesSequence",
		vr: "SQ",
		value: {
			type: "sequence",
			items: [[{
				tag: "(0020,000E)",
				keyword: "SeriesInstanceUID",
				vr: "UI",
				value: { type: "string", value: "nested-series" },
			}]],
		},
	},
];

describe("tag row shaping", () => {
	it("retains a sequence parent when a descendant matches the filter", () => {
		const rows = flattenTagRows(tags, "f42", new Set(), "nested-series");
		expect(rows.map((row) => row.node.keyword)).toEqual(["ReferencedSeriesSequence"]);
	});

	it("uses stable nested keys and depth for expanded sequences", () => {
		const rows = flattenTagRows(tags, "f42", new Set(["f42-1"]), "");
		expect(rows.map(({ key, depth }) => ({ key, depth }))).toEqual([
			{ key: "f42-0", depth: 0 },
			{ key: "f42-1", depth: 0 },
			{ key: "f42-1:item0-0", depth: 1 },
		]);
	});

	it("formats long, binary, and truncated values consistently", () => {
		const longRow = {
			key: "long",
			depth: 0,
			node: {
				tag: "(0010,0010)",
				keyword: "PatientName",
				vr: "PN",
				value: { type: "string" as const, value: "x".repeat(90) },
			},
		};
		expect(tagValueDisplay(longRow, false)).toBe(`${"x".repeat(80)}…`);
		expect(tagValueDisplay(longRow, true)).toBe("x".repeat(90));
		expect(tagValueToCopyText({
			type: "numbers",
			value: [1, 2],
			total: 5,
			truncated: true,
		})).toBe("1, 2 (first 2 of 5)");
	});
});
