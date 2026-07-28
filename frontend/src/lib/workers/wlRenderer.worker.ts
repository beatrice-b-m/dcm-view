/// <reference lib="webworker" />

import { renderRawFrameToRgba } from "../rawWindowing";
import type {
	WlRendererFailure,
	WlRendererRequest,
	WlRendererSuccess,
} from "./wlRendererProtocol";

self.onmessage = async (event: MessageEvent<WlRendererRequest>) => {
	const payload = event.data;
	if (!payload || payload.type !== "render") {
		return;
	}

	try {
		const { id, metadata, wc, ww } = payload;
		const width = metadata.columns;
		const height = metadata.rows;
		const output = renderRawFrameToRgba(
			{ metadata, buffer: payload.buffer },
			wc,
			ww,
		);
		const bitmap = await createImageBitmap(new ImageData(output, width, height));
		const response: WlRendererSuccess = { type: "rendered", id, width, height, bitmap };
		self.postMessage(
			response,
			[bitmap as unknown as Transferable],
		);
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		const response: WlRendererFailure = { type: "error", id: payload.id, message };
		self.postMessage(response);
	}
};
