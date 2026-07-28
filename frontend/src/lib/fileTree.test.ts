import { describe, expect, it } from "vitest";
import type { FileSummary } from "../api";
import { buildFileTree, fileMatchesFilter, patientDetailWithCounts } from "./fileTree";

function file(overrides: Partial<FileSummary>): FileSummary {
	return {
		index: 0,
		path: "/study/image.dcm",
		label: "image",
		patient_id: "P-1",
		patient_name: "DOE^JANE",
		study_instance_uid: "1.2.3.study",
		study_date: "20260728",
		study_description: "Chest",
		series_instance_uid: "1.2.3.series",
		series_number: "1",
		series_description: "Axial",
		modality: "CT",
		instance_number: "1",
		sop_instance_uid: "1.2.3.image",
		has_pixels: true,
		frame_count: 1,
		rows: 512,
		columns: 512,
		transfer_syntax_uid: "1.2.840.10008.1.2.1",
		default_window: null,
		...overrides,
	};
}

describe("file tree shaping", () => {
	it("groups by patient, study, and series while sorting numeric instances", () => {
		const tree = buildFileTree([
			file({ index: 42, instance_number: "10", path: "/study/ten.dcm" }),
			file({ index: 3, instance_number: "2", path: "/study/two.dcm" }),
			file({
				index: 900,
				series_instance_uid: "1.2.3.other",
				series_number: "2",
				series_description: "Coronal",
			}),
		]);

		expect(tree).toHaveLength(1);
		expect(tree[0].studies).toHaveLength(1);
		expect(tree[0].studies[0].series).toHaveLength(2);
		expect(tree[0].studies[0].series[0].files.map((item) => item.file.index))
			.toEqual([3, 42]);
		expect(patientDetailWithCounts(tree[0])).toContain("3 images");
	});

	it("supports scoped terms and combines terms with AND semantics", () => {
		const image = file({});
		expect(fileMatchesFilter(image, "patient:jane modality:ct")).toBe(true);
		expect(fileMatchesFilter(image, "study:chest modality:mr")).toBe(false);
		expect(fileMatchesFilter(image, "series:axial")).toBe(true);
	});

	it("uses file identifiers for fallback keys without assuming density", () => {
		const tree = buildFileTree([
			file({
				index: 77,
				patient_id: "",
				patient_name: "",
				study_instance_uid: "",
				study_description: "",
				study_date: "",
				series_instance_uid: "",
				series_number: "",
				series_description: "",
			}),
		]);
		expect(tree[0].key).toBe("patient:file-77");
		expect(tree[0].studies[0].key).toContain("study:file-77");
		expect(tree[0].studies[0].series[0].key).toContain("series:file-77");
	});
});
