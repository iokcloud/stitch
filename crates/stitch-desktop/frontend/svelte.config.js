import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Tauri v2: https://v2.tauri.app/start/frontend/sveltekit/
    // SSG via adapter-static — no Node server (Tauri cannot host SSR).
    // See also: https://svelte.dev/docs/kit/adapter-static
    adapter: adapter({
      pages: "build",
      assets: "build",
      // SSG: omit SPA fallback (only prerendered routes).
      precompress: false,
      strict: true,
    }),
  },
};

export default config;
