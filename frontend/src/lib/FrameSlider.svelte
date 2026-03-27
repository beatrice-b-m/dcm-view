<script lang="ts">
	import type { FileSummary } from "../api";

	let {
		files,
		activeFileIndex,
		currentFrame = $bindable(),
	}: {
		files: FileSummary[];
		activeFileIndex: number;
		currentFrame: number;
	} = $props();

	const activeFile = $derived(files[activeFileIndex]);

	function previous() {
		if (!activeFile) {
			return;
		}
		currentFrame = Math.max(0, currentFrame - 1);
	}

	function next() {
		if (!activeFile) {
			return;
		}
		currentFrame = Math.min(activeFile.frame_count - 1, currentFrame + 1);
	}
</script>

{#if activeFile && activeFile.frame_count > 1}
	<div class="slider">
		<button type="button" onclick={previous}>◀</button>
		<span>frame {currentFrame + 1} / {activeFile.frame_count}</span>
		<button type="button" onclick={next}>▶</button>
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
</style>
