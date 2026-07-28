<script lang="ts">
	import type { FileSummary } from "../api";

	let {
		activeFile,
		currentFrame = $bindable(),
	}: {
		activeFile: FileSummary;
		currentFrame: number;
	} = $props();

	let isPlaying = $state(false);
	let playTimer: ReturnType<typeof setInterval> | null = null;

	const FPS_OPTIONS = [1, 5, 10, 15, 24];
	let fps = $state(10);
	let sweepMode = $state(false);
	let playDirection: 1 | -1 = 1;

	function previous() {
		if (!activeFile || activeFile.frame_count <= 1) {
			return;
		}
		currentFrame = Math.max(0, currentFrame - 1);
	}

	function next() {
		if (!activeFile || activeFile.frame_count <= 1) {
			return;
		}
		currentFrame = Math.min(activeFile.frame_count - 1, currentFrame + 1);
	}

	function togglePlay() {
		if (!activeFile || activeFile.frame_count <= 1) return;
		if (!isPlaying) {
			playDirection = 1;
		}
		isPlaying = !isPlaying;
	}

	$effect(() => {
		if (activeFile && currentFrame >= activeFile.frame_count) {
			currentFrame = 0;
		}
		if (!activeFile || activeFile.frame_count <= 1) {
			isPlaying = false;
		}
	});

	$effect(() => {
		if (playTimer) {
			clearInterval(playTimer);
			playTimer = null;
		}

		if (!isPlaying || !activeFile || activeFile.frame_count <= 1) {
			return;
		}

		const intervalMs = 1000 / fps;
		playTimer = setInterval(() => {
			if (!activeFile) return;
			if (sweepMode) {
				const next = currentFrame + playDirection;
				if (next >= activeFile.frame_count || next < 0) {
					playDirection = playDirection === 1 ? -1 : 1;
				} else {
					currentFrame = next;
				}
			} else {
				currentFrame = (currentFrame + 1) % activeFile.frame_count;
			}
		}, intervalMs);

		return () => {
			if (playTimer) {
				clearInterval(playTimer);
				playTimer = null;
			}
		};
	});


	$effect(() => {
		const handleKey = (event: KeyboardEvent) => {
			if (!activeFile || activeFile.frame_count <= 1) {
				return;
			}

			const target = event.target as HTMLElement | null;
			if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) {
				return;
			}

			if (event.key === "ArrowLeft" || event.key === "[") {
				event.preventDefault();
				previous();
			}
			if (event.key === "ArrowRight" || event.key === "]") {
				event.preventDefault();
				next();
			}
			if (event.key === ' ') {
				event.preventDefault();
				togglePlay();
			}
		};

		window.addEventListener("keydown", handleKey);
		return () => window.removeEventListener("keydown", handleKey);
	});
</script>

{#if activeFile && activeFile.frame_count > 1}
	<div class="slider">
		<button type="button" onclick={previous}>◀</button>
		<span>frame {currentFrame + 1} / {activeFile.frame_count}</span>
		<button type="button" onclick={next}>▶</button>
		<button type="button" class="play" onclick={togglePlay}>
			{isPlaying ? "⏸" : "▶"}
		</button>
		<select class="fps-select" bind:value={fps}>
			{#each FPS_OPTIONS as f}
				<option value={f}>{f} fps</option>
			{/each}
		</select>
		<button type="button" class="mode-toggle" onclick={() => sweepMode = !sweepMode}>
			{sweepMode ? 'Sweep' : 'Loop'}
		</button>
	</div>
{/if}

<style>
	.slider {
		display: flex;
		flex-wrap: wrap;
		gap: 0.42rem 0.55rem;
		align-items: center;
		min-width: 0;
		padding: 0.55rem 0.85rem;
		background: var(--surface-chrome);
		border-top: 1px solid var(--border-subtle);
		color: var(--text-secondary);
		font-size: 0.84rem;
	}
	button {
		min-height: var(--control-height);
		background: var(--surface-control);
		border: 1px solid var(--border-subtle);
		color: var(--text-secondary);
		padding: 0.22rem 0.65rem;
		border-radius: var(--radius-control);
		cursor: pointer;
		font: inherit;
	}
	.play {
		margin-left: 0.25rem;
		border-color: rgba(10, 132, 255, 0.42);
		color: var(--text-primary);
	}
	.fps-select {
		min-height: var(--control-height);
		background: var(--surface-control);
		border: 1px solid var(--border-subtle);
		color: var(--text-primary);
		padding: 0.22rem 1.55rem 0.22rem 0.5rem;
		border-radius: var(--radius-control);
		font-size: inherit;
	}
	button:hover,
	.fps-select:hover {
		background: var(--surface-control-hover);
		color: var(--text-primary);
	}
	button:focus-visible,
	.fps-select:focus-visible {
		outline: none;
		box-shadow: var(--focus-ring);
	}
	.mode-toggle {
		font-size: 0.85em;
	}
</style>
