export interface WindowPreset {
	center: number;
	width: number;
}

export interface FileSummary {
	index: number;
	path: string;
	label: string;
	has_pixels: boolean;
	frame_count: number;
	rows: number;
	columns: number;
	transfer_syntax_uid: string;
	default_window: WindowPreset | null;
}

export interface FilesResponse {
	files: FileSummary[];
	tunnelled: boolean;
	tunnel_host: string | null;
	server_start_ms: number;
}

export interface FrameInfo {
	frame_count: number;
	rows: number;
	columns: number;
	transfer_syntax: string;
	has_pixels: boolean;
	default_window: WindowPreset | null;
}

export type TagValue =
	| { type: "string"; value: string }
	| { type: "number"; value: number }
	| { type: "numbers"; value: number[] }
	| { type: "binary"; length: number }
	| { type: "sequence"; items: TagNode[][] }
	| { type: "error"; message: string };

export interface TagNode {
	tag: string;
	vr: string;
	keyword: string;
	value: TagValue;
}

async function requestJson<T>(path: string): Promise<T> {
	const response = await fetch(path);
	if (!response.ok) {
		throw new Error(`Request failed (${response.status}): ${path}`);
	}
	return (await response.json()) as T;
}

export function fetchFiles(): Promise<FilesResponse> {
	return requestJson<FilesResponse>("/api/files");
}

export function fetchFrameInfo(fileIndex: number): Promise<FrameInfo> {
	return requestJson<FrameInfo>(`/api/file/${fileIndex}/info`);
}

export function fetchTags(fileIndex: number): Promise<TagNode[]> {
	return requestJson<TagNode[]>(`/api/file/${fileIndex}/tags`);
}

export function frameUrl(fileIndex: number, frame: number, wc?: number | null, ww?: number | null): string {
	const url = new URL(`/api/file/${fileIndex}/frame/${frame}`, window.location.origin);
	if (wc !== undefined && wc !== null) {
		url.searchParams.set("wc", String(wc));
	}
	if (ww !== undefined && ww !== null) {
		url.searchParams.set("ww", String(ww));
	}
	return `${url.pathname}${url.search}`;
}
