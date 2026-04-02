/// <reference lib="webworker" />

type RawFrameMetadata = {
	rows: number;
	columns: number;
	bitsAllocated: number;
	pixelRepresentation: number;
	samplesPerPixel: number;
	photometricInterpretation: string;
	rescaleSlope: number;
	rescaleIntercept: number;
	defaultWc: number | null;
	defaultWw: number | null;
};

type RenderMessage = {
	type: "render";
	id: number;
	metadata: RawFrameMetadata;
	buffer: ArrayBuffer;
	wc: number;
	ww: number;
};

function buildLut(
	bitsAllocated: number,
	pixelRepresentation: number,
	rescaleSlope: number,
	rescaleIntercept: number,
	wc: number,
	ww: number,
	invert: boolean,
): Uint8Array {
	const low = wc - ww / 2;
	const high = wc + ww / 2;
	const range = Math.max(high - low, 1e-10);

	let minRaw: number;
	let size: number;
	if (bitsAllocated === 8) {
		minRaw = 0;
		size = 256;
	} else if (pixelRepresentation === 1) {
		minRaw = -32768;
		size = 65536;
	} else {
		minRaw = 0;
		size = 65536;
	}

	const lut = new Uint8Array(size);
	for (let i = 0; i < size; i++) {
		const raw = i + minRaw;
		const modal = raw * rescaleSlope + rescaleIntercept;
		let val = (modal - low) / range;
		val = val < 0 ? 0 : val > 1 ? 1 : val;
		if (invert) val = 1 - val;
		lut[i] = Math.round(val * 255);
	}
	return lut;
}

self.onmessage = (event: MessageEvent<RenderMessage>) => {
	const payload = event.data;
	if (!payload || payload.type !== "render") {
		return;
	}

	try {
		const { id, metadata, wc, ww } = payload;
		const width = metadata.columns;
		const height = metadata.rows;
		const pixelCount = width * height;
		const output = new Uint8ClampedArray(pixelCount * 4);
		const invert = metadata.photometricInterpretation === "MONOCHROME1";
		const lut = buildLut(
			metadata.bitsAllocated,
			metadata.pixelRepresentation,
			metadata.rescaleSlope,
			metadata.rescaleIntercept,
			wc,
			Math.max(ww, 1),
			invert,
		);

		if (metadata.bitsAllocated === 8) {
			const source = new Uint8Array(payload.buffer);
			for (let i = 0; i < pixelCount; i++) {
				const g = lut[source[i]];
				const offset = i * 4;
				output[offset] = g;
				output[offset + 1] = g;
				output[offset + 2] = g;
				output[offset + 3] = 255;
			}
		} else if (metadata.pixelRepresentation === 1) {
			const source = new Int16Array(payload.buffer);
			for (let i = 0; i < pixelCount; i++) {
				const g = lut[source[i] + 32768];
				const offset = i * 4;
				output[offset] = g;
				output[offset + 1] = g;
				output[offset + 2] = g;
				output[offset + 3] = 255;
			}
		} else {
			const source = new Uint16Array(payload.buffer);
			for (let i = 0; i < pixelCount; i++) {
				const g = lut[source[i]];
				const offset = i * 4;
				output[offset] = g;
				output[offset + 1] = g;
				output[offset + 2] = g;
				output[offset + 3] = 255;
			}
		}

		self.postMessage(
			{ type: "rendered", id, width, height, rgba: output.buffer },
			[output.buffer],
		);
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		self.postMessage({ type: "error", id: payload.id, message });
	}
};
