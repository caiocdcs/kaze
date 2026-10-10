/** Stroke icon paths (24×24 viewBox), drawn to match the design canvas. */
export const icons = {
	library: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
	add: 'M12 5v14M5 12h14',
	wanted: 'M6 4h12v16l-6-4-6 4z',
	activity: 'M3 12h4l3-8 4 16 3-8h4',
	settings: 'M4 7h9M17 7h3M4 17h3M11 17h9M15 5v4M9 15v4',
	import: 'M3 6h6l2 2h10v12H3zM12 11v6M9 14l3 3 3-3',
	search: 'M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14zM20 20l-3.5-3.5',
	refresh: 'M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7',
	download: 'M12 4v11M7 10l5 5 5-5M5 20h14',
	close: 'M6 6l12 12M18 6L6 18',
	external: 'M14 4h6v6M20 4l-9 9M18 14v6H4V6h6',
	check: 'M5 12l5 5 9-10',
	play: 'M7 5l12 7-12 7z',
	copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
	trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3'
} as const;

export type IconName = keyof typeof icons;
