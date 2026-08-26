/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Backend base URL, derived by vite.config.ts from the root .env's SWARM_BIND. */
  readonly VITE_API_BASE: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
