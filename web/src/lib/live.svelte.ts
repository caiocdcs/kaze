import { SvelteMap } from 'svelte/reactivity';
import { api, type JobRecord } from './api/index.ts';
import type { Event } from './api/types/Event.ts';

export type Progress = { done: number; total: number };

/**
 * What is happening right now, kept current by one `EventSource` on
 * `/api/v1/events`. Pages read it, and re-load their own data when
 * `libraryVersion` moves.
 */
class Live {
	connected = $state(false);
	/** Queued and running jobs, newest first. */
	active = $state<JobRecord[]>([]);
	progress = new SvelteMap<number, Progress>();
	/** Bumped whenever chapters were found, downloaded or failed. */
	libraryVersion = $state(0);
	/** Bumped on every job event, for pages showing the queue. */
	jobsVersion = $state(0);
	wanted = $state(0);
	/** Names of the sources whose recent jobs keep failing. */
	failingSources = $state<string[]>([]);

	running = $derived(this.active.filter((j) => j.state === 'running'));
	queued = $derived(this.active.filter((j) => j.state === 'queued'));

	#source: EventSource | null = null;
	#retry = 1000;
	#refetch: ReturnType<typeof setTimeout> | null = null;
	#listeners = new Set<(e: Event) => void>();

	start() {
		if (this.#source) return;
		this.#connect();
		this.#reload();
	}

	/** Subscribe to raw events; returns the unsubscribe function. */
	on(listener: (e: Event) => void): () => void {
		this.#listeners.add(listener);
		return () => this.#listeners.delete(listener);
	}

	#connect() {
		const source = new EventSource(api.eventsUrl());
		this.#source = source;
		source.onopen = () => {
			this.connected = true;
			this.#retry = 1000;
			// Anything may have changed while we were away.
			this.#reload();
			this.libraryVersion++;
		};
		source.onmessage = (msg) => {
			try {
				this.#handle(JSON.parse(msg.data) as Event);
			} catch {
				// A malformed event is not worth breaking the stream for.
			}
		};
		source.onerror = () => {
			this.connected = false;
			source.close();
			this.#source = null;
			setTimeout(() => this.#connect(), this.#retry);
			this.#retry = Math.min(this.#retry * 2, 30_000);
		};
	}

	#handle(e: Event) {
		switch (e.type) {
			case 'job_progress':
				this.progress.set(e.job_id, { done: e.done, total: e.total });
				break;
			case 'job_finished':
			case 'job_failed':
			case 'job_cancelled':
				this.progress.delete(e.job_id);
				this.#scheduleReload();
				break;
			case 'job_queued':
			case 'job_started':
				this.#scheduleReload();
				break;
			case 'chapters_found':
			case 'chapter_downloaded':
			case 'chapter_failed':
				this.libraryVersion++;
				this.#scheduleReload();
				break;
		}
		for (const listener of this.#listeners) listener(e);
	}

	/** Batch bursts of events (a refresh can queue hundreds of jobs) into one fetch. */
	#scheduleReload() {
		if (this.#refetch) return;
		this.#refetch = setTimeout(() => {
			this.#refetch = null;
			this.#reload();
		}, 400);
	}

	async #reload() {
		try {
			const [jobs, series, sources] = await Promise.all([
				api.jobs(500),
				api.series(),
				api.sources()
			]);
			this.active = jobs.filter((j) => j.state === 'queued' || j.state === 'running');
			this.wanted = series.reduce((sum, s) => sum + s.wanted, 0);
			this.failingSources = sources.filter((s) => s.failing).map((s) => s.name);
			this.jobsVersion++;
		} catch {
			// Shown as "offline" through `connected`; the next event retries.
		}
	}
}

export const live = new Live();
