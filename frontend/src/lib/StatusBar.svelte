<script lang="ts">
	let {
		serverStartMs,
		fileCount,
		tunnelled,
		tunnelHost,
	}: {
		serverStartMs: number;
		fileCount: number;
		tunnelled: boolean;
		tunnelHost: string | null;
	} = $props();

	let nowMs = $state(Date.now());
	const timer = setInterval(() => {
		nowMs = Date.now();
	}, 1000);

	const uptime = $derived.by(() => {
		const elapsedSeconds = Math.max(0, Math.floor((nowMs - serverStartMs) / 1000));
		const hours = String(Math.floor(elapsedSeconds / 3600)).padStart(2, "0");
		const minutes = String(Math.floor((elapsedSeconds % 3600) / 60)).padStart(2, "0");
		const seconds = String(elapsedSeconds % 60).padStart(2, "0");
		return `${hours}:${minutes}:${seconds}`;
	});
</script>

<footer class="status">
	<span>{window.location.origin}</span>
	<span>{fileCount} files loaded</span>
	<span>
		uptime {uptime}
		{#if tunnelled}
			· tunnelled{#if tunnelHost} from {tunnelHost}{/if}
		{/if}
	</span>
</footer>

<style>
	.status {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
		font-size: 0.85rem;
		padding: 0.5rem 1rem;
		background: #242424;
		border-top: 1px solid #333;
	}
</style>
