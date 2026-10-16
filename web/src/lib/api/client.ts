import { auth } from '../auth.svelte.ts';

const BASE = '/api/v1';
const KEY_STORAGE = 'kaze.apiKey';

export class ApiError extends Error {
	constructor(
		readonly status: number,
		message: string
	) {
		super(message);
	}
}

export function apiKey(): string | null {
	try {
		return localStorage.getItem(KEY_STORAGE);
	} catch {
		return null;
	}
}

export function setApiKey(key: string | null) {
	try {
		if (key) localStorage.setItem(KEY_STORAGE, key);
		else localStorage.removeItem(KEY_STORAGE);
	} catch {
		// Private mode: the key lives only as long as the page.
	}
}

type Query = Record<string, string | number | boolean | undefined | null>;

export function url(path: string, query?: Query): string {
	const params = new URLSearchParams();
	for (const [k, v] of Object.entries(query ?? {})) {
		if (v !== undefined && v !== null) params.set(k, String(v));
	}
	// EventSource and <img> can't send headers, so the key also goes in the query.
	const key = apiKey();
	if (key && (path === '/events' || path.endsWith('/cover'))) params.set('apikey', key);
	const qs = params.toString();
	return `${BASE}${path}${qs ? `?${qs}` : ''}`;
}

async function send(
	method: string,
	path: string,
	body?: unknown,
	query?: Query
): Promise<Response> {
	const headers: Record<string, string> = { Accept: 'application/json' };
	const key = apiKey();
	if (key) headers['X-Api-Key'] = key;
	if (body !== undefined) headers['Content-Type'] = 'application/json';

	const res = await fetch(url(path, query), {
		method,
		headers,
		body: body === undefined ? undefined : JSON.stringify(body)
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

async function request<T>(method: string, path: string, body?: unknown, query?: Query): Promise<T> {
	const res = await send(method, path, body, query);
	if (res.status === 204) return undefined as T;
	const text = await res.text();
	return (text ? JSON.parse(text) : undefined) as T;
}

export const http = {
	get: <T>(path: string, query?: Query) => request<T>('GET', path, undefined, query),
	post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
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
