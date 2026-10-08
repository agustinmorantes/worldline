/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "true" replaces the Tauri backend with fixtures (`bun run dev:mock`). */
  readonly VITE_IPC_MOCKS?: string;
}
