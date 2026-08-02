// Tauri v2 + SvelteKit adapter-static (SSG)
// https://v2.tauri.app/start/frontend/sveltekit/
// https://svelte.dev/docs/kit/adapter-static
//
// prerender=true → emit static HTML/JS into build/ (SSG).
// ssr=false → required for Tauri: no Node SSR; APIs need `window`.
//   (Official Tauri checklist; load/Tauri calls stay in onMount / client.)

export const prerender = true;
export const ssr = false;
