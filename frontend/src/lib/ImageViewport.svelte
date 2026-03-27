<script lang="ts">
	import { frameUrl, type FileSummary } from "../api";

	let {
		files,
		activeFileIndex,
		currentFrame,
		windowCenter,
		windowWidth,
	}: {
		files: FileSummary[];
		activeFileIndex: number;
		currentFrame: number;
		windowCenter: number | null;
		windowWidth: number | null;
	} = $props();

	const activeFile = $derived(files[activeFileIndex]);
	const src = $derived(activeFile ? frameUrl(activeFile.index, currentFrame, windowCenter, windowWidth) : "");
</script>

<section class="viewport">
	{#if !activeFile}
		<div class="placeholder">No file selected</div>
	{:else if !activeFile.has_pixels}
		<div class="placeholder">No pixel data</div>
	{:else}
		<img src={src} alt={`frame ${currentFrame + 1}`} draggable="false" />
	{/if}
</section>

<style>
	.viewport {
		display: grid;
		place-items: center;
		background: #111;
		min-height: 0;
	}
	img {
		max-width: 100%;
		max-height: 100%;
		object-fit: contain;
	}
	.placeholder {
		color: #9a9a9a;
	}
</style>
