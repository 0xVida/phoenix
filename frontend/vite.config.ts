import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

export default defineConfig(({ mode }) => {
  // Same repo-root .env that scripts/swarm.sh loads (not frontend/.env) —
  // one source of truth for which port the backend actually binds to, so the
  // dashboard can never point at a stale/hardcoded port. Mirrors swarm.sh's
  // own `BIND="${SWARM_BIND:-0.0.0.0:3000}"; PORT="${BIND##*:}"` exactly.
  // Empty prefix means loadEnv returns every var (not just VITE_-prefixed) —
  // safe here because only the derived API base is ever injected into
  // client code below; provider keys never leave this Node-side config file.
  const rootEnv = loadEnv(mode, path.resolve(__dirname, ".."), "");
  const bind = rootEnv["SWARM_BIND"] || "0.0.0.0:3000";
  const port = bind.split(":").pop();
  const apiBase = rootEnv["VITE_API_BASE"] || `http://localhost:${port}`;

  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      tsconfigPaths: true,
    },
    define: {
      "import.meta.env.VITE_API_BASE": JSON.stringify(apiBase),
    },
  };
});
