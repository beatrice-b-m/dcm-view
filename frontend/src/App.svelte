<script lang="ts">
	import { onMount } from "svelte";
	import { fetchFiles, type FilesResponse, type WindowMode } from "./api";
	import FileTabs from "./lib/FileTabs.svelte";
	import FrameSlider from "./lib/FrameSlider.svelte";
	import ImageViewport from "./lib/ImageViewport.svelte";
	import StatusBar from "./lib/StatusBar.svelte";
	import TagPanel from "./lib/TagPanel.svelte";
	import ViewerToolbar from "./lib/ViewerToolbar.svelte";
	import { WL_PRESETS, type ActiveTool } from "./lib/viewerTools";

	let filesResponse = $state<FilesResponse | null>(null);
	let loadError = $state<string | null>(null);

	let activeFileIndex = $state(0);
	let currentFrame = $state(0);
	let windowCenter = $state<number | null>(null);
	let windowWidth = $state<number | null>(null);
	let activeTool = $state<ActiveTool>('window_level');
	let windowMode = $state<WindowMode>('default');
	let selectedPresetId = $state('default');
	let resetCount = $state(0);

	function resetViewport() {
		windowCenter = null;
		windowWidth = null;
		windowMode = 'default';
		selectedPresetId = 'default';
		resetCount += 1;
	}

	$effect(() => {
		const preset = WL_PRESETS.find(p => p.id === selectedPresetId);
		if (!preset) return;
		if (preset.wc !== undefined && preset.ww !== undefined) {
			windowCenter = preset.wc;
			windowWidth = preset.ww;
			windowMode = 'default';
		} else {
			windowCenter = null;
			windowWidth = null;
			windowMode = preset.mode ?? 'default';
		}
	});

	$effect(() => {
		const handleKey = (event: KeyboardEvent) => {
			const target = event.target as HTMLElement | null;
			if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;
			switch (event.key.toLowerCase()) {
				case 'w': activeTool = 'window_level'; break;
				case 'p': activeTool = 'pan'; break;
				case 'z': activeTool = 'zoom'; break;
				case 's': activeTool = 'scroll'; break;
			}
		};
		window.addEventListener('keydown', handleKey);
		return () => window.removeEventListener('keydown', handleKey);
	});

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
			bind:windowMode
			bind:selectedPresetId
		/>
		<ViewerToolbar
			bind:activeTool
			bind:selectedPresetId
			onreset={resetViewport}
		/>
		<section class="content">
			<ImageViewport
				files={filesResponse.files}
				activeFileIndex={activeFileIndex}
				bind:currentFrame
				bind:windowCenter
				bind:windowWidth
				activeTool={activeTool}
				windowMode={windowMode}
				resetCount={resetCount}
				onreset={resetViewport}
			/>
			<TagPanel files={filesResponse.files} activeFileIndex={activeFileIndex} />
		</section>
		<FrameSlider
			files={filesResponse.files}
			activeFileIndex={activeFileIndex}
			bind:currentFrame
			windowCenter={windowCenter}
			windowWidth={windowWidth}
			windowMode={windowMode}
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
		grid-template-rows: auto auto 1fr auto auto;
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
