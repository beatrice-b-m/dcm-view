<script lang="ts">
	import { onMount } from "svelte";
	import { fetchFiles, type FilesResponse } from "./api";
	import FileTabs from "./lib/FileTabs.svelte";
	import FrameSlider from "./lib/FrameSlider.svelte";
	import ImageViewport from "./lib/ImageViewport.svelte";
	import StatusBar from "./lib/StatusBar.svelte";
	import TagPanel from "./lib/TagPanel.svelte";

	let filesResponse = $state<FilesResponse | null>(null);
	let loadError = $state<string | null>(null);

	let activeFileIndex = $state(0);
	let currentFrame = $state(0);
	let windowCenter = $state<number | null>(null);
	let windowWidth = $state<number | null>(null);

	onMount(async () => {
		try {
			filesResponse = await fetchFiles();
		} catch (error) {
			loadError = error instanceof Error ? error.message : String(error);
		}
	});
</script>

{#if loadError}
	<main class="error">{loadError}</main>
{:else if !filesResponse}
	<main class="loading">Loading dcmview…</main>
{:else}
	<main class="layout">
		<FileTabs
			files={filesResponse.files}
			bind:activeFileIndex
			bind:currentFrame
			bind:windowCenter
			bind:windowWidth
		/>
		<section class="content">
			<ImageViewport
				files={filesResponse.files}
				activeFileIndex={activeFileIndex}
				currentFrame={currentFrame}
				bind:windowCenter
				bind:windowWidth
			/>
			<TagPanel files={filesResponse.files} activeFileIndex={activeFileIndex} />
		</section>
		<FrameSlider
			files={filesResponse.files}
			activeFileIndex={activeFileIndex}
			bind:currentFrame
			windowCenter={windowCenter}
			windowWidth={windowWidth}
		/>
		<StatusBar
			serverStartMs={filesResponse.server_start_ms}
			fileCount={filesResponse.files.length}
			tunnelled={filesResponse.tunnelled}
			tunnelHost={filesResponse.tunnel_host}
		/>
	</main>
{/if}

<style>
	:global(body) {
		margin: 0;
		font-family: system-ui, sans-serif;
		background: #1a1a1a;
		color: #e0e0e0;
	}

	.layout {
		display: grid;
		grid-template-rows: auto 1fr auto auto;
		height: 100vh;
	}

	.content {
		display: grid;
		grid-template-columns: 1fr minmax(320px, 420px);
		min-height: 0;
	}

	.loading,
	.error {
		display: grid;
		place-content: center;
		height: 100vh;
	}
</style>
