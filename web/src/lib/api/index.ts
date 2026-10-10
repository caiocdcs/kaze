import { http, url } from './client.ts';
import type { ChapterRecord } from './types/ChapterRecord.ts';
import type { Config } from './types/Config.ts';
import type { HealthCheck } from './types/HealthCheck.ts';
import type { Integration } from './types/Integration.ts';
import type { IntegrationInput } from './types/IntegrationInput.ts';
import type { Preset } from './types/Preset.ts';
import type { Trigger } from './types/Trigger.ts';
import type { FollowRequest } from './types/FollowRequest.ts';
import type { HistoryEntry } from './types/HistoryEntry.ts';
import type { JobRecord } from './types/JobRecord.ts';
import type { Lookup } from './types/Lookup.ts';
import type { SeriesDetail } from './types/SeriesDetail.ts';
import type { SeriesSummary } from './types/SeriesSummary.ts';
import type { SettingsView } from './types/SettingsView.ts';
import type { Settings } from './types/Settings.ts';
import type { SourceInfo } from './types/SourceInfo.ts';
import type { SourceResults } from './types/SourceResults.ts';
import type { Task } from './types/Task.ts';
import type { TaskInfo } from './types/TaskInfo.ts';
import type { UpdateSeries } from './types/UpdateSeries.ts';
import type { WantedChapter } from './types/WantedChapter.ts';

export { ApiError, apiKey, setApiKey } from './client.ts';

/** Every endpoint of `/api/v1`, typed with the structs ts-rs exports. */
export const api = {
	sources: () => http.get<SourceInfo[]>('/sources'),
	search: (q: string, source?: string) => http.get<SourceResults[]>('/search', { q, source }),
	lookup: (u: string) => http.get<Lookup>('/lookup', { url: u }),

	series: () => http.get<SeriesSummary[]>('/series'),
	seriesDetail: (id: number) => http.get<SeriesDetail>(`/series/${id}`),
	follow: (req: FollowRequest) => http.post<{ id: number }>('/series', req),
	updateSeries: (id: number, req: UpdateSeries) => http.patch<SeriesDetail>(`/series/${id}`, req),
	unfollow: (id: number) => http.del<void>(`/series/${id}`),
	refreshSeries: (id: number) => http.post<{ job_id: number | null }>(`/series/${id}/refresh`),
	coverUrl: (id: number) => url(`/series/${id}/cover`),

	setMonitored: (ids: number[], monitored: boolean) =>
		http.patch<{ updated: number }>('/chapters', { ids, monitored }),
	downloadChapter: (id: number) => http.post<{ job_id: number | null }>(`/chapters/${id}/download`),
	wanted: () => http.get<WantedChapter[]>('/wanted'),
	downloadWanted: () => http.post<{ queued: number }>('/wanted/download'),

	jobs: (limit?: number) => http.get<JobRecord[]>('/jobs', { limit }),
	cancelJob: (id: number) => http.post<void>(`/jobs/${id}/cancel`),
	retryJob: (id: number) => http.post<void>(`/jobs/${id}/retry`),
	history: (limit?: number) => http.get<HistoryEntry[]>('/history', { limit }),

	settings: () => http.get<SettingsView>('/settings'),
	/** `null` resets a setting to its default. */
	updateSettings: (changes: { [K in keyof Settings]?: Settings[K] | null }) =>
		http.patch<SettingsView>('/settings', changes),
	tasks: () => http.get<TaskInfo[]>('/tasks'),
	runTask: (task: Task) => http.post<void>(`/tasks/${task}/run`),
	health: () => http.get<HealthCheck[]>('/health'),

	integrations: () => http.get<Integration[]>('/integrations'),
	presets: () => http.get<Preset[]>('/integrations/presets'),
	createIntegration: (input: IntegrationInput) => http.post<Integration>('/integrations', input),
	replaceIntegration: (id: number, input: IntegrationInput) =>
		http.put<Integration>(`/integrations/${id}`, input),
	deleteIntegration: (id: number) => http.del<void>(`/integrations/${id}`),
	testIntegration: (id: number) => http.post<Integration>(`/integrations/${id}/test`),
	testConfig: (config: Config) => http.post<void>('/integrations/test', config),

	eventsUrl: () => url('/events')
};

export type {
	ChapterRecord,
	Config,
	HealthCheck,
	Integration,
	IntegrationInput,
	Preset,
	Trigger,
	HistoryEntry,
	JobRecord,
	Lookup,
	SeriesDetail,
	SeriesSummary,
	SettingsView,
	Settings,
	SourceInfo,
	SourceResults,
	TaskInfo,
	WantedChapter
};
