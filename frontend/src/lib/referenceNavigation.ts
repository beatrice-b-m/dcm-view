import type {
	FileSummary,
	ReferenceMatchSummary,
	ReferenceTargetSummary,
} from "../api";

export type ReferenceDestination = {
	file: FileSummary;
	frameIndex: number;
};

export function referenceIdentity(target: ReferenceTargetSummary): string {
	if (target.sop_instance_uid) {
		return `SOP Instance ${target.sop_instance_uid}`;
	}
	if (target.series_instance_uid) {
		return `Series ${target.series_instance_uid}`;
	}
	if (target.sop_class_uid) {
		return `SOP Class ${target.sop_class_uid}`;
	}
	return "Identity unavailable";
}

export function referenceDetails(target: ReferenceTargetSummary): string[] {
	const details: string[] = [];
	if (target.frame_numbers.length > 0) {
		details.push(`frame ${target.frame_numbers.join(", ")}`);
	}
	if (target.segment_numbers.length > 0) {
		details.push(`segment ${target.segment_numbers.join(", ")}`);
	}
	return details;
}

export function referenceDestination(
	match: ReferenceMatchSummary,
	files: readonly FileSummary[],
): ReferenceDestination | null {
	const file = files.find((candidate) => candidate.index === match.file_index);
	if (!file || file.frame_count <= 0) return null;

	const candidates = match.frame_indices.length > 0 ? match.frame_indices : [0];
	const frameIndex = candidates.find(
		(candidate) => Number.isInteger(candidate) && candidate >= 0 && candidate < file.frame_count,
	);
	return frameIndex === undefined ? null : { file, frameIndex };
}
