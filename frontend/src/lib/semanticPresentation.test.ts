import { describe, expect, it } from "vitest";
import {
	codedConceptLabel,
	mappingFormula,
	semanticKindLabel,
	semanticModeLabel,
	supportsSemanticContext,
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

	it("shows semantic controls only for supported derived image types", () => {
		expect(supportsSemanticContext("segmentation", "seg")).toBe(true);
		expect(supportsSemanticContext("parametric_map", "pm")).toBe(true);
		expect(supportsSemanticContext("radiation_therapy", "1.2.840.10008.5.1.4.1.1.481.2")).toBe(true);
		expect(supportsSemanticContext("classic_image", "ct")).toBe(false);
		expect(supportsSemanticContext("radiation_therapy", "1.2.840.10008.5.1.4.1.1.481.1")).toBe(false);
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
