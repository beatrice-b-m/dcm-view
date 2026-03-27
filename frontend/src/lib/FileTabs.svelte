<script lang="ts">
	import type { FileSummary } from "../api";

	let {
		files,
		activeFileIndex = $bindable(),
		currentFrame = $bindable(),
		windowCenter = $bindable(),
		windowWidth = $bindable(),
	}: {
		files: FileSummary[];
		activeFileIndex: number;
		currentFrame: number;
		windowCenter: number | null;
		windowWidth: number | null;
	} = $props();

	function activate(index: number) {
		activeFileIndex = index;
		currentFrame = 0;
		windowCenter = null;
		windowWidth = null;
	}
</script>

<header class="tabs">
	<div class="title">dcmview</div>
	<div class="tab-strip">
		{#each files as file}
			<button
				type="button"
				class:active={file.index === activeFileIndex}
				onclick={() => activate(file.index)}
			>
				{file.label}
			</button>
		{/each}
	</div>
</header>

<style>
	.tabs {
		display: flex;
		gap: 1rem;
		align-items: center;
		background: #242424;
		padding: 0.75rem 1rem;
		border-bottom: 1px solid #333;
	}
	.title {
		font-weight: 700;
	}
	.tab-strip {
		display: flex;
		gap: 0.5rem;
		overflow-x: auto;
	}
	button {
		background: transparent;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 6px;
		padding: 0.35rem 0.75rem;
		white-space: nowrap;
	}
	button.active {
		border-color: #4a9eff;
		color: #fff;
	}
</style>
