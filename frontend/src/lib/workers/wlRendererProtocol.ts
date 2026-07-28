import type { RawFrameMetadata } from "../../rawFrame";

export type WlRendererRequest = {
	type: "render";
	id: number;
	metadata: RawFrameMetadata;
	buffer: ArrayBuffer;
	wc: number;
	ww: number;
};

export type WlRendererSuccess = {
	type: "rendered";
	id: number;
	width: number;
	height: number;
	bitmap: ImageBitmap;
};

export type WlRendererFailure = {
	type: "error";
	id: number;
	message: string;
};

export type WlRendererResponse = WlRendererSuccess | WlRendererFailure;
