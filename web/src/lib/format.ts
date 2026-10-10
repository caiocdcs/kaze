import type { JobRecord } from './api/index.ts';

const nf = new Intl.NumberFormat('en-US');
export const num = (n: number) => nf.format(n);

const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
	['year', 365 * 24 * 3600],
	['month', 30 * 24 * 3600],
	['week', 7 * 24 * 3600],
	['day', 24 * 3600],
	['hour', 3600],
	['minute', 60]
];

/** "2 days ago", "in 3 hours", "just now". */
export function ago(iso: string | null | undefined): string {
	if (!iso) return '—';
	const secs = (new Date(iso).getTime() - Date.now()) / 1000;
	for (const [unit, size] of UNITS) {
		if (Math.abs(secs) >= size) return rtf.format(Math.round(secs / size), unit);
	}
	return 'just now';
}

export function dateTime(iso: string | null | undefined): string {
	return iso ? new Date(iso).toLocaleString() : '—';
}

export function every(secs: number): string {
	if (secs % 3600 === 0) return secs === 3600 ? 'every hour' : `every ${secs / 3600} hours`;
	if (secs % 60 === 0) return secs === 60 ? 'every minute' : `every ${secs / 60} minutes`;
	return `every ${secs}s`;
}

/** A short human label for what a job does. */
export function jobLabel(j: JobRecord): string {
	const series = j.series_title ?? 'Unknown series';
	switch (j.job.type) {
		case 'download_chapter':
			return `${series} · Ch.${j.chapter ?? '?'}`;
		case 'refresh_series':
			return `${series} · check for chapters`;
		case 'sync_latest':
			return `Latest updates on ${j.job.source}`;
	}
}

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));
