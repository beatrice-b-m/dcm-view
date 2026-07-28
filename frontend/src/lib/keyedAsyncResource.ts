export type AsyncResourceStatus = "idle" | "loading" | "ready" | "error";

export type AsyncResourceSnapshot<Value> = {
	status: AsyncResourceStatus;
	value: Value | undefined;
	error: string | null;
	generation: number;
};

export type KeyedAsyncResourceOptions<Key, Value> = {
	load: (key: Key) => Promise<Value>;
	onChange?: (key: Key, snapshot: AsyncResourceSnapshot<Value>) => void;
	errorMessage?: (error: unknown) => string;
};

type InFlight<Value> = {
	generation: number;
	promise: Promise<Value>;
};

function defaultErrorMessage(error: unknown): string {
	return error instanceof Error && error.message ? error.message : String(error);
}

/**
 * Keeps independent async state by logical key and rejects stale completions
 * using a monotonically increasing generation for each key.
 */
export class KeyedAsyncResource<Key, Value> {
	readonly #load: (key: Key) => Promise<Value>;
	readonly #onChange?: (key: Key, snapshot: AsyncResourceSnapshot<Value>) => void;
	readonly #errorMessage: (error: unknown) => string;
	readonly #states = new Map<Key, AsyncResourceSnapshot<Value>>();
	readonly #inFlight = new Map<Key, InFlight<Value>>();

	constructor({ load, onChange, errorMessage = defaultErrorMessage }: KeyedAsyncResourceOptions<Key, Value>) {
		this.#load = load;
		this.#onChange = onChange;
		this.#errorMessage = errorMessage;
	}

	get(key: Key): AsyncResourceSnapshot<Value> {
		return this.#states.get(key) ?? {
			status: "idle",
			value: undefined,
			error: null,
			generation: 0,
		};
	}

	ensure(key: Key): Promise<Value> {
		const state = this.#states.get(key);
		if (state?.status === "ready" && state.value !== undefined) {
			return Promise.resolve(state.value);
		}
		const pending = this.#inFlight.get(key);
		if (pending) return pending.promise;
		return this.#start(key);
	}

	reload(key: Key): Promise<Value> {
		return this.#start(key);
	}

	invalidate(key: Key): void {
		const generation = this.get(key).generation + 1;
		const snapshot: AsyncResourceSnapshot<Value> = {
			status: "idle",
			value: undefined,
			error: null,
			generation,
		};
		this.#states.set(key, snapshot);
		this.#inFlight.delete(key);
		this.#onChange?.(key, snapshot);
	}

	#start(key: Key): Promise<Value> {
		const previous = this.get(key);
		const generation = previous.generation + 1;
		const loading: AsyncResourceSnapshot<Value> = {
			status: "loading",
			value: previous.value,
			error: null,
			generation,
		};
		this.#states.set(key, loading);
		this.#onChange?.(key, loading);

		const promise = this.#load(key)
			.then((value) => {
				if (this.get(key).generation === generation) {
					const ready: AsyncResourceSnapshot<Value> = {
						status: "ready",
						value,
						error: null,
						generation,
					};
					this.#states.set(key, ready);
					this.#onChange?.(key, ready);
				}
				return value;
			})
			.catch((error: unknown) => {
				if (this.get(key).generation === generation) {
					const failed: AsyncResourceSnapshot<Value> = {
						status: "error",
						value: previous.value,
						error: this.#errorMessage(error),
						generation,
					};
					this.#states.set(key, failed);
					this.#onChange?.(key, failed);
				}
				throw error;
			})
			.finally(() => {
				if (this.#inFlight.get(key)?.generation === generation) {
					this.#inFlight.delete(key);
				}
			});
		this.#inFlight.set(key, { generation, promise });
		return promise;
	}
}
