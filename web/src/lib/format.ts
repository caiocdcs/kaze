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

const languages = new Intl.DisplayNames(['en'], { type: 'language' });
/** "English", "Brazilian Portuguese"; the tag itself if the browser doesn't know it. */
export function languageName(tag: string): string {
	try {
		return languages.of(tag) ?? tag;
	} catch {
		return tag;
	}
}

/** Every language the sources offer, English first. */
export function offeredLanguages(sources: { languages: string[] }[]): string[] {
	const all = new Set(sources.flatMap((s) => s.languages));
	return [...all].sort((a, b) =>
		a === 'en' ? -1 : b === 'en' ? 1 : languageName(a).localeCompare(languageName(b))
	);
}

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
/** 1.2 GB, 340 MB: disk usage. */
export function bytes(n: number): string {
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	let i = 0;
	while (n >= 1000 && i < units.length - 1) {
		n /= 1000;
		i++;
	}
	return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

export function jobLabel(j: JobRecord): string {
	const series = j.series_title ?? 'Unknown series';
	switch (j.job.type) {
		case 'download_chapter':
			return `${series} · Ch.${j.chapter ?? '?'}`;
		case 'refresh_series':
			return `${series} · check for chapters`;
		case 'sync_latest':
			return `Latest updates on ${j.job.source}`;
		case 'notify':
			return `Notify: ${j.job.message.title}`;
		case 'scan_library':
			return 'Library server scan';
		case 'measure_library':
			return 'Measure library';
	}
}

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));
