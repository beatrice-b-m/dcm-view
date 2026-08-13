<script lang="ts">
	import type { FileSummary } from "../api";
	import {
		buildDirectoryTree,
		buildFileTree,
		fileAriaLabel,
		fileMatchesFilter,
		nodeAriaLabel,
		patientDetailWithCounts,
		seriesDetailWithCounts,
		studyDetailWithCounts,
		tierLabel,
		type NavKind,
		type DirectoryNode,
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
	let viewMode = $state<"study" | "directory">("study");

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
	const directoryTree = $derived(buildDirectoryTree(filteredFiles));
</script>

{#snippet nodeContent(kind: NavKind, label: string, detail: string)}
	<span class="kind-badge">{tierLabel(kind)}</span>
	<span class="node-text">
		<span class="node-label">{label}</span>
		{#if detail}<span class="node-detail">{detail}</span>{/if}
	</span>
{/snippet}

{#snippet directoryNodes(nodes: DirectoryNode[], depth: number)}
	{#each nodes as node}
		{#if node.kind === "folder"}
			<button
				type="button"
				class="directory-row folder-row"
				style:--depth={depth}
				aria-expanded={!isCollapsed(node.key)}
				onclick={() => toggleNode(node.key)}
			>
				<span class="folder-twisty">{isCollapsed(node.key) ? "›" : "⌄"}</span>
				<span class="folder-icon" aria-hidden="true"></span>
				<span class="directory-label">{node.label}</span>
				<span class="directory-count">{node.children.length}</span>
			</button>
			{#if !isCollapsed(node.key)}
				{@render directoryNodes(node.children, depth + 1)}
			{/if}
		{:else}
			<button
				type="button"
				class="directory-row directory-file"
				class:active={node.file.index === activeFileIndex}
				style:--depth={depth}
				title={node.file.path}
				onclick={() => onopenfile(node.file.index)}
			>
				<span></span><span class="file-icon" aria-hidden="true">DCM</span>
				<span class="directory-label">{node.label}</span>
			</button>
		{/if}
	{/each}
{/snippet}

<aside class="navigator" class:collapsed>
	<div class="navigator-header">
		{#if !collapsed}
			<div class="header-copy"><strong>Explorer</strong><span>{files.length} images</span></div>
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
		<div class="view-switch" role="group" aria-label="Explorer organization">
			<button class:active={viewMode === "study"} onclick={() => viewMode = "study"}>Study</button>
			<button class:active={viewMode === "directory"} onclick={() => viewMode = "directory"}>Directory</button>
		</div>
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
		{#if viewMode === "study"}
		<div class="tree study-tree" role="tree" aria-label="DICOM file hierarchy">
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
		{:else}
			<div class="tree directory-tree" role="tree" aria-label="Directory file hierarchy">
				{@render directoryNodes(directoryTree, 0)}
			</div>
		{/if}
	{/if}
</aside>

<style>
	.navigator {
		display: grid;
		grid-template-rows: auto auto auto 1fr;
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

	.header-copy { display: grid; gap: 0.08rem; }
	.header-copy strong { color: var(--text-primary); font-size: 0.82rem; }
	.header-copy span { color: var(--text-muted); font-size: 0.64rem; font-weight: 500; }

	.view-switch {
		display: grid;
		grid-template-columns: 1fr 1fr;
		margin: 0.55rem 0.65rem 0;
		padding: 0.18rem;
		border: 1px solid var(--border-subtle);
		border-radius: 0.48rem;
		background: rgba(0, 0, 0, 0.18);
	}
	.view-switch button {
		height: 1.8rem;
		border: 0;
		border-radius: 0.34rem;
		background: transparent;
		color: var(--text-muted);
		font: 650 0.7rem var(--font-ui);
		cursor: pointer;
	}
	.view-switch button.active {
		background: var(--surface-control-hover);
		color: var(--text-primary);
		box-shadow: 0 1px 4px rgba(0, 0, 0, 0.24);
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

	.study-tree { padding: 0.5rem 0.55rem 0.8rem; }
	.study-tree .tree-group {
		margin-bottom: 0.55rem;
		border: 1px solid var(--border-subtle);
		border-radius: 0.52rem;
		background: rgba(255, 255, 255, 0.018);
		overflow: hidden;
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
	.study-tree .depth-0 {
		min-height: 2.9rem;
		background: linear-gradient(90deg, rgba(67, 155, 255, 0.13), transparent 82%);
		border-left: 3px solid var(--accent);
	}
	.study-tree .depth-1 { border-left: 3px solid rgba(143, 108, 255, 0.64); }
	.study-tree .depth-2 { border-left: 3px solid rgba(69, 191, 154, 0.58); }
	.study-tree .depth-1,
	.study-tree .depth-2 { border-top: 1px solid rgba(255, 255, 255, 0.045); }

	.file-row.active {
		background: var(--accent-soft);
		color: var(--text-primary);
		box-shadow: inset 3px 0 0 var(--accent);
	}

	.depth-0 { padding-left: 0.48rem; }
	.depth-1 { padding-left: 0.9rem; }
	.depth-2 { padding-left: 1.35rem; }
	.depth-3 { padding-left: 2.82rem; padding-right: 0.65rem; }

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

	.directory-tree { padding: 0.35rem 0.5rem 0.75rem; }
	.directory-row {
		display: grid;
		grid-template-columns: 0.9rem 1.6rem minmax(0, 1fr) auto;
		align-items: center;
		gap: 0.32rem;
		width: 100%;
		min-height: 2rem;
		padding: 0.2rem 0.42rem 0.2rem calc(0.42rem + var(--depth) * 0.75rem);
		border: 0;
		border-radius: 0.32rem;
		background: transparent;
		color: var(--text-secondary);
		font: 0.76rem var(--font-ui);
		text-align: left;
		cursor: pointer;
	}
	.directory-row:hover { background: rgba(255, 255, 255, 0.05); }
	.directory-row.active { background: var(--accent-soft); box-shadow: inset 3px 0 var(--accent); color: var(--text-primary); }
	.folder-twisty { color: var(--text-muted); font-size: 1rem; text-align: center; }
	.folder-icon {
		width: 1.05rem; height: 0.72rem; border-radius: 0.13rem;
		background: #caa85b; position: relative; opacity: 0.9;
	}
	.folder-icon::before { content: ""; position: absolute; left: 0.08rem; top: -0.22rem; width: 0.48rem; height: 0.25rem; border-radius: 0.1rem 0.1rem 0 0; background: #d9ba70; }
	.file-icon { color: #73b9ff; font: 700 0.49rem var(--font-ui); letter-spacing: 0.02em; }
	.directory-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.directory-count { color: var(--text-muted); font-size: 0.64rem; }
</style>
