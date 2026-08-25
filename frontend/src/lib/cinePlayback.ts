export type CineMode = "loop" | "sweep";
export type CineDirection = 1 | -1;

export type CineStep = {
	frame: number;
	direction: CineDirection;
};

export function nextCineStep(
	currentFrame: number,
	totalFrames: number,
	mode: CineMode,
	direction: CineDirection,
): CineStep {
	if (totalFrames <= 1) return { frame: 0, direction };
	if (mode === "loop") {
		return {
			frame: (currentFrame + direction + totalFrames) % totalFrames,
			direction,
		};
	}

	let nextDirection = direction;
	let frame = currentFrame + nextDirection;
	if (frame >= totalFrames || frame < 0) {
		nextDirection = nextDirection === 1 ? -1 : 1;
		frame = currentFrame + nextDirection;
	}
	return { frame, direction: nextDirection };
}

export function buildCineLookahead(
	currentFrame: number,
	totalFrames: number,
	mode: CineMode,
	direction: CineDirection,
	maxFrames: number,
): number[] {
	const targets: number[] = [];
	const seen = new Set<number>([currentFrame]);
	let step: CineStep = { frame: currentFrame, direction };
	const stepLimit = Math.max(maxFrames * 2, totalFrames * 2);
	for (let index = 0; index < stepLimit && targets.length < maxFrames; index += 1) {
		step = nextCineStep(step.frame, totalFrames, mode, step.direction);
		if (seen.has(step.frame)) {
			if (mode === "loop") break;
			continue;
		}
		seen.add(step.frame);
		targets.push(step.frame);
		if (seen.size >= totalFrames) break;
	}
	return targets;
}

export function cineFrameIntervalMs(fps: number): number {
	return 1000 / Math.max(1, fps);
}

export type RenderPacedCineOptions = {
	initialFrame: number;
	totalFrames: number;
	mode: CineMode;
	direction: CineDirection;
	fps: number;
	signal: AbortSignal;
	now: () => number;
	waitForDelay: (delayMs: number, signal: AbortSignal) => Promise<boolean>;
	prepareFrame: (frame: number) => Promise<unknown>;
	presentFrame: (step: CineStep, signal: AbortSignal) => Promise<boolean>;
};

export async function runRenderPacedCine({
	initialFrame,
	totalFrames,
	mode,
	direction: initialDirection,
	fps,
	signal,
	now,
	waitForDelay,
	prepareFrame,
	presentFrame,
}: RenderPacedCineOptions): Promise<void> {
	let currentFrame = initialFrame;
	let direction = initialDirection;
	let lastPresentation = now();
	while (!signal.aborted) {
		const step = nextCineStep(currentFrame, totalFrames, mode, direction);
		const delay = lastPresentation + cineFrameIntervalMs(fps) - now();
		const [, deadlineReached] = await Promise.all([
			prepareFrame(step.frame),
			waitForDelay(delay, signal),
		]);
		if (!deadlineReached || signal.aborted) return;
		if (!await presentFrame(step, signal)) return;
		currentFrame = step.frame;
		direction = step.direction;
		lastPresentation = now();
	}
}
