<script lang="ts">
	import { frameUrl, type FileSummary } from "../api";

	let {
		files,
		activeFileIndex,
		currentFrame = $bindable(),
		windowCenter,
		windowWidth,
	}: {
		files: FileSummary[];
		activeFileIndex: number;
		currentFrame: number;
		windowCenter: number | null;
		windowWidth: number | null;
	} = $props();

	let isPlaying = $state(false);
	let playTimer: ReturnType<typeof setInterval> | null = null;

	const activeFile = $derived(files[activeFileIndex]);

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
		if (!activeFile || activeFile.frame_count <= 1) {
			return;
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

		playTimer = setInterval(() => {
			currentFrame = (currentFrame + 1) % activeFile.frame_count;
		}, 100);

		return () => {
			if (playTimer) {
				clearInterval(playTimer);
				playTimer = null;
			}
		};
	});

	$effect(() => {
		if (!activeFile || activeFile.frame_count <= 1) {
			return;
		}
		for (const step of [1, 2]) {
			const targetFrame = currentFrame + step;
			if (targetFrame >= activeFile.frame_count) {
				continue;
			}
			const prefetchUrl = frameUrl(activeFile.index, targetFrame, windowCenter, windowWidth);
			void fetch(prefetchUrl).catch(() => {});
		}
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
	</div>
{/if}

<style>
	.slider {
		display: flex;
		gap: 0.75rem;
		align-items: center;
		padding: 0.6rem 1rem;
		background: #242424;
		border-top: 1px solid #333;
	}
	button {
		background: #1b1b1b;
		border: 1px solid #3a3a3a;
		color: #e0e0e0;
		padding: 0.25rem 0.7rem;
		border-radius: 6px;
	}
	.play {
		margin-left: 0.25rem;
		border-color: #4a9eff;
	}
</style>
