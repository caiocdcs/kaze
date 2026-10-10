import type { ChapterRecord } from './api/index.ts';

/** Decimal-aware: 10 after 9.5, 12.15 before 12.5. */
export function compareNumbers(a: string, b: string): number {
	const [aw, af = ''] = a.split('.');
	const [bw, bf = ''] = b.split('.');
	return Number(aw) - Number(bw) || Number(`0.${af || 0}`) - Number(`0.${bf || 0}`);
}

export const newestFirst = (a: ChapterRecord, b: ChapterRecord) =>
	compareNumbers(b.number, a.number);

export function chapterName(c: { number: string; volume: number | null }): string {
	return c.volume !== null ? `Vol.${c.volume} Ch.${c.number}` : `Ch.${c.number}`;
}
