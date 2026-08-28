type PendingRequest<Value> = {
	controller: AbortController;
	promise: Promise<Value>;
};

export class SharedRequestRegistry<Key, Value> {
	readonly #pending = new Map<Key, PendingRequest<Value>>();

	get(key: Key): Promise<Value> | undefined {
		return this.#pending.get(key)?.promise;
	}

	request(key: Key, load: (signal: AbortSignal) => Promise<Value>): Promise<Value> {
		const existing = this.#pending.get(key);
		if (existing) return existing.promise;

		const controller = new AbortController();
		const promise = load(controller.signal).finally(() => {
			if (this.#pending.get(key)?.promise === promise) {
				this.#pending.delete(key);
			}
		});
		this.#pending.set(key, { controller, promise });
		return promise;
	}

	abortAll(): void {
		for (const request of this.#pending.values()) {
			request.controller.abort();
		}
		this.#pending.clear();
	}
}
