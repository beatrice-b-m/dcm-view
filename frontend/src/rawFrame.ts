export interface RawFrameMetadata {
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
}

export interface RawFrame {
	metadata: RawFrameMetadata;
	buffer: ArrayBuffer;
}
