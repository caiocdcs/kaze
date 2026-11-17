import { auth } from '../auth.svelte.ts';

import { resolve } from '$app/paths';

// `resolve('/')` carries the URL base kaze is served under, if any.
const BASE = `${resolve('/').replace(/\/$/, '')}/api/v1`;
// Where kaze UIs before sign-in kept the API key.
const LEGACY_KEY = 'kaze.apiKey';

export class ApiError extends Error {
	constructor(
		readonly status: number,
		message: string
	) {
		super(message);
	}
}

/**
 * The API key an older kaze UI saved in this browser: sent once to create
 * the account after an upgrade, so it needn't be typed in.
 */
export function legacyKeyHeader(): Record<string, string> {
	try {
		const key = localStorage.getItem(LEGACY_KEY);
		return key ? { 'X-Api-Key': key } : {};
	} catch {
		return {};
	}
}

export function forgetLegacyKey() {
	try {
		localStorage.removeItem(LEGACY_KEY);
	} catch {
		// Private mode: nothing was kept.
	}
}

type Query = Record<string, string | number | boolean | undefined | null>;

export function url(path: string, query?: Query): string {
	const params = new URLSearchParams();
	for (const [k, v] of Object.entries(query ?? {})) {
		if (v !== undefined && v !== null) params.set(k, String(v));
	}
	const qs = params.toString();
	return `${BASE}${path}${qs ? `?${qs}` : ''}`;
}

async function send(
	method: string,
	path: string,
	body?: unknown,
	query?: Query,
	extra: Record<string, string> = {}
): Promise<Response> {
	// The session cookie goes along by itself.
	const headers: Record<string, string> = { Accept: 'application/json', ...extra };
	// A file goes as it is; anything else as JSON.
	const raw = body instanceof Blob;
	if (body !== undefined)
		headers['Content-Type'] = raw ? 'application/octet-stream' : 'application/json';

	const res = await fetch(url(path, query), {
		method,
		headers,
		body: body === undefined ? undefined : raw ? body : JSON.stringify(body)
	});
	if (res.status === 401) auth.needed = true;
	if (!res.ok) {
		let message = res.statusText;
		try {
			const err = await res.json();
			if (typeof err?.error === 'string') message = err.error;
		} catch {
			// Not JSON; keep the status text.
		}
		throw new ApiError(res.status, message);
	}
	return res;
}

async function request<T>(
	method: string,
	path: string,
	body?: unknown,
	query?: Query,
	headers?: Record<string, string>
): Promise<T> {
	const res = await send(method, path, body, query, headers);
	if (res.status === 204) return undefined as T;
	const text = await res.text();
	return (text ? JSON.parse(text) : undefined) as T;
}

export const http = {
	get: <T>(path: string, query?: Query) => request<T>('GET', path, undefined, query),
	post: <T>(path: string, body?: unknown, headers?: Record<string, string>) =>
		request<T>('POST', path, body, undefined, headers),
	patch: <T>(path: string, body: unknown) => request<T>('PATCH', path, body),
	put: <T>(path: string, body: unknown) => request<T>('PUT', path, body),
	del: <T>(path: string) => request<T>('DELETE', path),
	/** Fetch a file and hand it to the browser as a download. */
	save: async (path: string) => {
		const res = await send('GET', path);
		const name = /filename="([^"]+)"/.exec(res.headers.get('Content-Disposition') ?? '')?.[1];
		const link = document.createElement('a');
		link.href = URL.createObjectURL(await res.blob());
		link.download = name ?? 'download';
		link.click();
		URL.revokeObjectURL(link.href);
	}
};
