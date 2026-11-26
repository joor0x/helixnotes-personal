import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig, type DevEnvironment, type Plugin } from "vite";

const host = process.env.TAURI_DEV_HOST;

const SVELTE_STYLE_ID = /\.svelte\?svelte&type=style&lang\.css$/;

// In dev, a component's virtual style module can be requested before the
// component itself has been compiled (e.g. on the webview's first load). Its
// CSS is then not cached yet, Vite falls back to the raw .svelte source, and
// Tailwind fails with "Invalid declaration". Compile the component first.
function compileSvelteBeforeStyle(): Plugin {
  return {
    name: "helix:compile-svelte-before-style",
    apply: "serve",
    enforce: "pre",
    load: {
      filter: { id: SVELTE_STYLE_ID },
      async handler(id) {
        const filename = id.slice(0, id.indexOf("?"));
        if (!this.getModuleInfo(filename)?.meta.svelte?.css) {
          // apply: "serve" guarantees a dev environment here.
          await (this.environment as DevEnvironment).transformRequest(filename);
        }
        return null;
      },
    },
  };
}

export default defineConfig({
  plugins: [compileSvelteBeforeStyle(), sveltekit(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
          overlay: false,
        }
      : { overlay: false },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
