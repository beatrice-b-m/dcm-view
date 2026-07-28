export type FrameDirection = 1 | -1;

export type DisplayPrefetchPlan = {
	startFrame: number;
	totalFrames: number;
	direction: FrameDirection;
	currentBlobBytes: number;
	fullStackBudgetBytes: number;
	nearDistance: number;
	forwardOnly?: boolean;
	lookaheadFrames?: number;
};

export function buildDirectionalFrameOrder(
	centerFrame: number,
	totalFrames: number,
	maxDistance: number,
	direction: FrameDirection,
): number[] {
	const result: number[] = [];
	const distanceCap = Math.min(Math.max(totalFrames - 1, 0), Math.max(maxDistance, 0));
	for (let delta = 1; delta <= distanceCap; delta += 1) {
		const preferred = centerFrame + delta * direction;
		const secondary = centerFrame - delta * direction;
		if (preferred >= 0 && preferred < totalFrames) result.push(preferred);
		if (secondary >= 0 && secondary < totalFrames) result.push(secondary);
	}
	return result;
}

export function shouldPrefetchWholeDisplayStack(
	totalFrames: number,
	frameBytes: number,
	budgetBytes: number,
): boolean {
	if (totalFrames <= 1 || frameBytes <= 0 || budgetBytes <= 0) return false;
	return totalFrames * frameBytes <= budgetBytes;
}

export function planDisplayPrefetchTargets({
	startFrame,
	totalFrames,
	direction,
	currentBlobBytes,
	fullStackBudgetBytes,
	nearDistance,
	forwardOnly = false,
	lookaheadFrames = 0,
}: DisplayPrefetchPlan): number[] {
	if (totalFrames <= 1) return [];
	if (forwardOnly) {
		const targets: number[] = [];
		for (let delta = 1; delta <= lookaheadFrames; delta += 1) {
			const frame = startFrame + delta * direction;
			if (frame >= 0 && frame < totalFrames) targets.push(frame);
		}
		return targets;
	}

	const fullStack = shouldPrefetchWholeDisplayStack(
		totalFrames,
		currentBlobBytes,
		fullStackBudgetBytes,
	);
	return buildDirectionalFrameOrder(
		startFrame,
		totalFrames,
		fullStack ? totalFrames - 1 : nearDistance,
		direction,
	);
}
