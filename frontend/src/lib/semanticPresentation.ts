import type { CodedConceptSummary, SemanticContext } from "../generated/api-types";
import type { SemanticContextResponse } from "../generated/api-types";

export type SemanticMode = "pixel_preview" | "semantic_context";

export interface SegmentationOverlaySelection {
	segmentationFileIndex: number;
	segmentationFrameIndex: number;
	sourceFileIndex: number;
	sourceFrameIndex: number;
}

export function segmentationOverlaySelection(
	response: SemanticContextResponse | null,
	frameIndex: number,
): SegmentationOverlaySelection | null {
	if (!response || response.context.kind !== "segmentation") return null;
	const mapping = response.context.frame_mappings.find(
		(candidate) => candidate.frame_index === frameIndex,
	);
	if (!mapping || mapping.mapping_status !== "resolved" || mapping.source_frames.length !== 1) {
		return null;
	}
	const source = mapping.source_frames[0];
	return {
		segmentationFileIndex: response.source_file_index,
		segmentationFrameIndex: frameIndex,
		sourceFileIndex: source.file_index,
		sourceFrameIndex: source.frame_index,
	};
}

const RT_DOSE_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.481.2";

export function supportsSemanticContext(objectKind: string, sopClassUid: string): boolean {
	return objectKind === "segmentation"
		|| objectKind === "parametric_map"
		|| sopClassUid === RT_DOSE_SOP_CLASS_UID;
}

export function semanticModeLabel(mode: SemanticMode): string {
	return mode === "pixel_preview" ? "Pixel Preview" : "Semantic Context";
}

export function semanticKindLabel(context: SemanticContext): string {
	switch (context.kind) {
		case "segmentation":
			return "Segmentation";
		case "parametric_map":
			return "Parametric Map";
		case "rt_dose":
			return "RT Dose";
		case "not_applicable":
			return "Generic image";
	}
}

export function codedConceptLabel(code: CodedConceptSummary | null): string {
	if (!code) return "Not declared";
	const identity = [code.value, code.scheme].filter(Boolean).join(" · ");
	return identity.length > 0 ? `${code.meaning} (${identity})` : code.meaning;
}

export function mappingFormula(slope: number | null, intercept: number | null): string | null {
	if (slope === null && intercept === null) return null;
	const resolvedSlope = slope ?? 1;
	const resolvedIntercept = intercept ?? 0;
	return `mapped = stored × ${resolvedSlope} + ${resolvedIntercept}`;
}
