import type { RawFrameMetadata } from "./generated/api-types";

export type { RawFrameMetadata } from "./generated/api-types";

export interface RawFrame {
	metadata: RawFrameMetadata;
	buffer: ArrayBuffer;
}
