/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_SPOTIFY_CLIENT_ID?: string;
	readonly VITE_TAURI_UPDATER_ENABLED?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}
