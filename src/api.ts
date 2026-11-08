import { invoke } from "@tauri-apps/api/core";

export type SystemInfo = {
  os: string;
  arch: string;
  hostname: string;
  total_ram_mb: number;
  used_ram_mb: number;
  cpu_brand: string;
  cpu_cores: number;
};

export type JavaRuntime = {
  path: string;
  version: string;
  major: number;
  vendor: string;
};

export type JvmPlan = {
  required_major: number;
  heap_mb: number;
  flags: string[];
  rationale: string;
  matched: JavaRuntime | null;
};

export type Instance = {
  id: string;
  name: string;
  minecraftVersion: string;
  loader: string;
  loaderVersion?: string | null;
  profile: string;
  createdAt: string;
  lastPlayed?: string | null;
  playtimeSeconds: number;
};

export type VersionInfo = {
  id: string;
  type: string;
  url: string;
  time: string;
  releaseTime: string;
};

export type ServerStatus = {
  host: string;
  port: number;
  online: boolean;
  pingMs?: number | null;
  motd?: string | null;
  playersOnline?: number | null;
  playersMax?: number | null;
  version?: string | null;
  favicon?: string | null;
  error?: string | null;
};

export type CuratedServer = {
  name: string;
  host: string;
  port: number;
  tags: string[];
  region: string;
};

export type Account = {
  uuid: string;
  name: string;
  clientId: string;
};

export type SpotifyProfile = {
  id: string;
  displayName: string;
  email?: string | null;
  product?: string | null;
  imageUrl?: string | null;
};

export type WorldSave = {
  name: string;
  path: string;
  modifiedAt?: number | null;
};

export type ScanFinding = {
  path: string;
  sha256: string;
  blocked: boolean;
  reasons: string[];
};

export type PlaytimeSummary = {
  lifetimeSeconds: number;
  weekSeconds: number;
  sessions: number;
};

export type Schematic = {
  title: string;
  prompt: string;
  palette: string[];
  layers: string[][];
};

export const api = {
  system: () => invoke<SystemInfo>("get_system_info"),
  javas: () => invoke<JavaRuntime[]>("list_java_runtimes"),
  planJvm: (minecraftVersion: string) =>
    invoke<JvmPlan>("plan_jvm", { minecraftVersion }),
  downloadTemurin: (feature: number) => invoke<string>("download_temurin", { feature }),
  instances: () => invoke<Instance[]>("list_instances"),
  upsertInstance: (instance: Partial<Instance> & { name: string; minecraftVersion: string; loader: string; profile: string }) =>
    invoke<Instance>("upsert_instance", { instance }),
  deleteInstance: (id: string) => invoke("delete_instance", { id }),
  worlds: (instanceId: string) => invoke<WorldSave[]>("list_worlds", { instanceId }),
  versions: () => invoke<{ latest: { release: string; snapshot: string }; versions: VersionInfo[] }>("fetch_version_manifest"),
  profiles: () => invoke<Array<{ id: string; name: string; loader: string; minecraft: string; mods: Array<{ slug: string; name: string; why: string }>; notes: string }>>("performance_profiles"),
  searchMods: (query: string, facets?: string) => invoke<{ hits: ModHit[] }>("search_modrinth", { query, facets }),
  searchCurseForgeMods: (query: string, minecraftVersion: string, loader: string, apiKey: string) =>
    invoke<Array<{ id: number; name: string; slug: string; summary: string; downloadCount: number; logoUrl?: string | null }>>(
      "search_curseforge_mods",
      { query, minecraftVersion, loader, apiKey },
    ),
  curseForgeFiles: (modId: number, minecraftVersion: string, loader: string, apiKey: string) =>
    invoke<Array<{ id: number; fileName: string }>>("curseforge_mod_files", {
      modId,
      minecraftVersion,
      loader,
      apiKey,
    }),
  installCurseForgeFile: (instanceId: string, fileId: number, apiKey: string) =>
    invoke<string>("install_curseforge_file", { instanceId, fileId, apiKey }),
  modVersions: (projectId: string, minecraftVersion: string, loader: string) =>
    invoke<Array<{ id: string; version_number: string }>>("modrinth_versions", {
      projectId,
      minecraftVersion,
      loader,
    }),
  downloadMod: (instanceId: string, versionId: string) =>
    invoke<string>("download_modrinth_version", { instanceId, versionId }),
  ping: (host: string, port?: number) => invoke<ServerStatus>("ping_server", { host, port }),
  servers: () => invoke<CuratedServer[]>("curated_servers"),
  startMsLogin: () => invoke<{
    userCode: string;
    deviceCode: string;
    verificationUri: string;
    interval: number;
    message: string;
    clientId: string;
  }>("ms_device_code_start"),
  pollMsLogin: (deviceCode: string, clientId: string) =>
    invoke<Account>("ms_device_code_poll", { deviceCode, clientId }),
  accounts: () => invoke<Account[]>("list_accounts"),
  activeAccount: () => invoke<Account | null>("active_account"),
  setActive: (uuid: string) => invoke("set_active_account", { uuid }),
  removeAccount: (uuid: string) => invoke("remove_account", { uuid }),
  setPin: (pin: string) => invoke("set_pin", { pin }),
  clearPin: (pin: string) => invoke("clear_pin", { pin }),
  changePin: (currentPin: string, newPin: string) =>
    invoke("change_pin", { currentPin, newPin }),
  verifyPin: (pin: string) => invoke<boolean>("verify_pin", { pin }),
  hasPin: () => invoke<boolean>("has_pin"),
  scanMods: (instanceId: string) => invoke<ScanFinding[]>("scan_instance_mods", { instanceId }),
  mixPacks: (packA: string, packB: string, output: string) =>
    invoke<{ output: string; files: number }>("mix_resource_packs", { packA, packB, output }),
  schematic: (prompt: string) => invoke<Schematic>("generate_schematic", { prompt }),
  playtime: () => invoke<PlaytimeSummary>("playtime_summary"),
  install: (versionId: string) => invoke<string>("install_version", { versionId }),
  installInstance: (instanceId: string) => invoke<string>("install_instance", { instanceId }),
  launch: (instanceId: string, serverHost?: string, serverPort?: number) =>
    invoke<{ pid: number; command: string }>("launch_instance", {
      instanceId,
      serverHost: serverHost ?? null,
      serverPort: serverPort ?? null,
    }),
  spotifyAuthorizeUrl: (clientId: string) => invoke<string>("spotify_authorize_url", { clientId }),
  spotifyAccessToken: () => invoke<string>("spotify_access_token"),
  spotifyCurrentUser: () => invoke<SpotifyProfile>("spotify_current_user"),
  spotifyDisconnect: () => invoke("spotify_disconnect"),
  saveCrosshair: (dataUrl: string) => invoke<string>("save_crosshair_png", { dataUrl }),
};

export type ModHit = {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  downloads: number;
  icon_url?: string;
  categories?: string[];
};
