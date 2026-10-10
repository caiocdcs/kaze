export type Toast = { id: number; message: string; tone: 'info' | 'error' };

class Toasts {
	list = $state<Toast[]>([]);
	#next = 0;

	show(message: string, tone: Toast['tone'] = 'info') {
		const id = this.#next++;
		this.list.push({ id, message, tone });
		setTimeout(() => this.dismiss(id), tone === 'error' ? 8000 : 4000);
	}

	error(e: unknown) {
		this.show(e instanceof Error ? e.message : String(e), 'error');
	}

	dismiss(id: number) {
		this.list = this.list.filter((t) => t.id !== id);
	}
}

export const toasts = new Toasts();
