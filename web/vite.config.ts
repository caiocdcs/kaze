import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			// A single-page app: kaze serves index.html for every path that isn't a file.
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	server: {
		// `just dev`: the UI on Vite, the API on `kaze serve`.
		proxy: { '/api': 'http://127.0.0.1:6531' }
	}
});
