<script lang="ts">
	import type { FileSummary } from "../api";
	import {
		buildFileTree,
		fileAriaLabel,
		fileMatchesFilter,
		nodeAriaLabel,
		patientDetailWithCounts,
		seriesDetailWithCounts,
		studyDetailWithCounts,
		tierLabel,
		type NavKind,
	} from "./fileTree";

	let {
		files,
		activeFileIndex,
		scanComplete = true,
		collapsed = $bindable(),
		onopenfile,
	}: {
		files: FileSummary[];
		activeFileIndex: number | null;
		scanComplete?: boolean;
		collapsed: boolean;
		onopenfile: (index: number) => void;
	} = $props();

	const LARGE_TREE_COLLAPSE_THRESHOLD = 500;
	let collapsedNodes = $state<Record<string, boolean>>({});
	let filterQuery = $state("");

	function defaultCollapsed(key: string): boolean {
		if (filterActive) {
			return false;
		}
		const scaleCollapseActive = !scanComplete || files.length > LARGE_TREE_COLLAPSE_THRESHOLD;
		if (!scaleCollapseActive) {
			return false;
		}
		if (!key.includes("/")) {
			return tree.length > 1;
		}
		return key.includes("/study:") || key.includes("/series:");
	}

	function isCollapsed(key: string): boolean {
		if (filterActive) {
			return false;
		}
		return collapsedNodes[key] ?? defaultCollapsed(key);
	}

	function toggleNode(key: string) {
		collapsedNodes = { ...collapsedNodes, [key]: !isCollapsed(key) };
	}

	const filterActive = $derived(filterQuery.trim().length > 0);
	const filteredFiles = $derived.by(() => {
		if (!filterActive) return files;
		return files.filter((file) => fileMatchesFilter(file, filterQuery));
	});

	const tree = $derived(buildFileTree(filteredFiles));
</script>

{#snippet nodeContent(kind: NavKind, label: string, detail: string)}
	<span class="kind-badge">{tierLabel(kind)}</span>
	<span class="node-text">
		<span class="node-label">{label}</span>
		{#if detail}<span class="node-detail">{detail}</span>{/if}
	</span>
{/snippet}

<aside class="navigator" class:collapsed>
	<div class="navigator-header">
		{#if !collapsed}
			<span>Files</span>
		{/if}
		<button
			type="button"
			class="collapse-button"
			onclick={() => collapsed = !collapsed}
			aria-label={collapsed ? "Expand file navigator" : "Collapse file navigator"}
			aria-expanded={!collapsed}
		>
			{collapsed ? "▶" : "◀"}
		</button>
	</div>

	{#if !collapsed}
		<div class="navigator-filter">
			<input
				class="filter-input"
				type="search"
				bind:value={filterQuery}
				placeholder="Patient, study, series, modality"
				aria-label="Filter file hierarchy"
			/>
			{#if filterActive}
				<div class="filter-result">showing {filteredFiles.length} of {files.length} images</div>
			{/if}
			{#if !scanComplete}
				<div class="scan-progress">indexed {files.length} file{files.length === 1 ? "" : "s"}...</div>
			{/if}
		</div>
		<div class="tree" role="tree" aria-label="DICOM file hierarchy">
			{#each tree as patient}
				{@const patientDetail = patientDetailWithCounts(patient)}
				<section class="tree-group">
					<button
						type="button"
						class="tree-header depth-0"
						aria-label={nodeAriaLabel(patient.kind, patient.label, patientDetail, isCollapsed(patient.key))}
						aria-expanded={!isCollapsed(patient.key)}
						onclick={() => toggleNode(patient.key)}
					>
						<span class="twisty">{isCollapsed(patient.key) ? "▶" : "▼"}</span>
						{@render nodeContent(patient.kind, patient.label, patientDetail)}
					</button>
					{#if !isCollapsed(patient.key)}
						{#each patient.studies as study}
							{@const studyDetail = studyDetailWithCounts(study)}
							<button
								type="button"
								class="tree-header depth-1"
								aria-label={nodeAriaLabel(study.kind, study.label, studyDetail, isCollapsed(study.key))}
								aria-expanded={!isCollapsed(study.key)}
								onclick={() => toggleNode(study.key)}
							>
								<span class="twisty">{isCollapsed(study.key) ? "▶" : "▼"}</span>
								{@render nodeContent(study.kind, study.label, studyDetail)}
							</button>
							{#if !isCollapsed(study.key)}
								{#each study.series as series}
									{@const seriesDetail = seriesDetailWithCounts(series)}
									<button
										type="button"
										class="tree-header depth-2"
										aria-label={nodeAriaLabel(series.kind, series.label, seriesDetail, isCollapsed(series.key))}
										aria-expanded={!isCollapsed(series.key)}
										onclick={() => toggleNode(series.key)}
									>
										<span class="twisty">{isCollapsed(series.key) ? "▶" : "▼"}</span>
										{@render nodeContent(series.kind, series.label, seriesDetail)}
									</button>
									{#if !isCollapsed(series.key)}
										{#each series.files as item}
											<button
												type="button"
												class="file-row depth-3"
												class:active={item.file.index === activeFileIndex}
												onclick={() => onopenfile(item.file.index)}
												title={item.file.path}
												aria-label={fileAriaLabel(item)}
											>
												{@render nodeContent(item.kind, item.label, item.detail)}
											</button>
										{/each}
									{/if}
								{/each}
							{/if}
						{/each}
					{/if}
				</section>
			{/each}
		</div>
	{/if}
</aside>

<style>
	.navigator {
		display: grid;
		grid-template-rows: auto auto 1fr;
		min-width: 0;
		min-height: 0;
		background: var(--surface-panel);
		border-right: 1px solid var(--border-subtle);
		overflow: hidden;
	}

	.navigator.collapsed {
		background: var(--surface-chrome);
	}

	.navigator-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
		min-height: 2.5rem;
		padding: 0.55rem 0.65rem;
		border-bottom: 1px solid var(--border-subtle);
		color: var(--text-secondary);
		font-size: 0.82rem;
		font-weight: 650;
	}

	.collapse-button {
		display: grid;
		place-items: center;
		width: 1.6rem;
		height: 1.6rem;
		border: 1px solid var(--border-subtle);
		border-radius: var(--radius-control);
		background: var(--surface-control);
		color: var(--text-secondary);
		cursor: pointer;
	}

	.collapse-button:hover {
		background: var(--surface-control-hover);
		color: var(--text-primary);
	}

	.collapse-button:focus-visible,
	.filter-input:focus-visible,
	.tree-header:focus-visible,
	.file-row:focus-visible {
		outline: none;
		box-shadow: inset var(--focus-ring);
	}

	.navigator-filter {
		display: grid;
		gap: 0.35rem;
		padding: 0.5rem 0.65rem;
		border-bottom: 1px solid var(--border-subtle);
	}

	.filter-input {
		width: 100%;
		height: var(--control-height);
		min-width: 0;
		border: 1px solid var(--border-subtle);
		border-radius: var(--radius-control);
		background: var(--surface-control);
		color: var(--text-primary);
		font: 0.78rem var(--font-ui);
		padding: 0 0.55rem;
	}

	.filter-input::placeholder {
		color: var(--text-muted);
	}

	.filter-result,
	.scan-progress {
		color: var(--text-muted);
		font-size: 0.72rem;
		line-height: 1.25;
	}

	.tree {
		overflow: auto;
		padding: 0.4rem 0;
		scrollbar-width: thin;
	}

	.tree-group,
	.tree-header,
	.file-row {
		min-width: 0;
	}

	.tree-header,
	.file-row {
		width: 100%;
		border: 0;
		background: transparent;
		color: var(--text-secondary);
		text-align: left;
		cursor: pointer;
	}

	.tree-header {
		display: grid;
		grid-template-columns: 1.1rem 3.35rem minmax(0, 1fr);
		align-items: start;
		gap: 0.35rem;
		padding-top: 0.28rem;
		padding-bottom: 0.28rem;
		font-size: 0.81rem;
	}

	.file-row {
		display: grid;
		grid-template-columns: 3.35rem minmax(0, 1fr);
		align-items: start;
		gap: 0.35rem;
		padding-top: 0.26rem;
		padding-bottom: 0.26rem;
		font-size: 0.8rem;
	}

	.tree-header:hover,
	.file-row:hover {
		background: rgba(255, 255, 255, 0.05);
	}

	.file-row.active {
		background: var(--accent-soft);
		color: var(--text-primary);
		box-shadow: inset 3px 0 0 var(--accent);
	}

	.depth-0 { padding-left: 0.55rem; }
	.depth-1 { padding-left: 1.25rem; }
	.depth-2 { padding-left: 1.95rem; }
	.depth-3 { padding-left: 3.55rem; padding-right: 0.65rem; }

	.twisty {
		align-self: center;
		color: var(--text-muted);
		font-size: 0.72rem;
		line-height: 1.35;
	}

	.kind-badge {
		display: block;
		align-self: center;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-muted);
		font-size: 0.6rem;
		font-weight: 700;
		letter-spacing: 0.04em;
		line-height: 1.45;
		text-transform: uppercase;
	}

	.node-text {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.node-text {
		display: grid;
		gap: 0.04rem;
		line-height: 1.25;
	}

	.node-label {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-secondary);
	}

	.file-row.active .node-label {
		color: var(--text-primary);
	}

	.node-detail {
		color: var(--text-muted);
		font-size: 0.72rem;
	}
</style>
