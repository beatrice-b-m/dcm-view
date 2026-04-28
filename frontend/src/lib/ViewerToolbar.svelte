<script lang="ts">
	import type { ActiveTool } from './viewerTools';
	import { TOOL_LABELS, TOOL_ORDER, TOOL_SHORTCUTS, WL_PRESETS } from './viewerTools';

	let {
		activeTool = $bindable(),
		selectedPresetId = $bindable(),
		onreset,
		onflipH,
		onflipV,
		onrotateCW,
		onrotateCCW,
	}: {
		activeTool: ActiveTool;
		selectedPresetId: string;
		onreset: () => void;
		onflipH: () => void;
		onflipV: () => void;
		onrotateCW: () => void;
		onrotateCCW: () => void;
	} = $props();

	const tools: ActiveTool[] = TOOL_ORDER;
</script>

<div class="toolbar">
	<div class="tool-group">
		{#each tools as tool}
			<button
				type="button"
				class:active={activeTool === tool}
				onclick={() => { activeTool = tool; }}
				title="{TOOL_LABELS[tool]} ({TOOL_SHORTCUTS[tool]})"
			>
				{TOOL_LABELS[tool]}
			</button>
		{/each}
	</div>
	<span class="sep"></span>
	<select bind:value={selectedPresetId}>
		{#each WL_PRESETS as preset}
			<option value={preset.id}>{preset.label}</option>
		{/each}
	</select>
	<span class="sep"></span>
	<button type="button" onclick={onreset} title="Reset viewport (double-click)">Reset</button>
	<div class="tool-group transform-group">
		<button type="button" onclick={onflipH} title="Flip horizontal">↔</button>
		<button type="button" onclick={onflipV} title="Flip vertical">↕</button>
		<span class="sep"></span>
		<button type="button" onclick={onrotateCCW} title="Rotate 90° CCW">↺</button>
		<button type="button" onclick={onrotateCW} title="Rotate 90° CW">↻</button>
	</div>
</div>

<style>
	.toolbar {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.4rem 0.75rem;
		background: #242424;
		border-bottom: 1px solid #333;
	}
	.tool-group {
		display: flex;
		gap: 2px;
	}
	.transform-group {
		margin-left: auto;
	}
	button {
		background: #1b1b1b;
		border: 1px solid #3a3a3a;
		color: #e0e0e0;
		padding: 0.25rem 0.65rem;
		border-radius: 6px;
		cursor: pointer;
		font-size: 0.85rem;
	}
	button:hover {
		background: #2a2a2a;
	}
	button.active {
		border-color: #4a9eff;
		color: #4a9eff;
		background: rgba(74, 158, 255, 0.1);
	}
	select {
		background: #1b1b1b;
		border: 1px solid #3a3a3a;
		color: #e0e0e0;
		padding: 0.25rem 0.65rem;
		border-radius: 6px;
		cursor: pointer;
		font-size: 0.85rem;
	}
	.sep {
		width: 1px;
		height: 1.2rem;
		background: #3a3a3a;
		margin: 0 0.25rem;
	}
</style>
