import { describe, expect, it } from "vitest";
import {
	codedConceptLabel,
	mappingFormula,
	semanticKindLabel,
	semanticModeLabel,
} from "./semanticPresentation";

describe("semantic presentation labels", () => {
	it("makes the active interpretation mode explicit", () => {
		expect(semanticModeLabel("pixel_preview")).toBe("Pixel Preview");
		expect(semanticModeLabel("semantic_context")).toBe("Semantic Context");
	});

	it("does not promote non-semantic objects", () => {
		expect(semanticKindLabel({ kind: "not_applicable", reason: "not derived" })).toBe(
			"Generic image",
		);
	});

	it("renders declared codes and mappings without inventing missing values", () => {
		expect(
			codedConceptLabel({ value: "HU", scheme: "UCUM", meaning: "Hounsfield unit" }),
		).toBe("Hounsfield unit (HU · UCUM)");
		expect(codedConceptLabel(null)).toBe("Not declared");
		expect(mappingFormula(2, -1)).toBe("mapped = stored × 2 + -1");
		expect(mappingFormula(null, null)).toBeNull();
	});
});
