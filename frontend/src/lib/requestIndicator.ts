export function trackForegroundRequest<Value>(
	request: Promise<Value> | null | undefined,
	isCurrent: () => boolean,
	setPending: (pending: boolean) => void,
): void {
	if (!request) {
		if (isCurrent()) setPending(false);
		return;
	}

	if (isCurrent()) setPending(true);
	const settle = () => {
		if (isCurrent()) setPending(false);
	};
	void request.then(settle, settle);
}
