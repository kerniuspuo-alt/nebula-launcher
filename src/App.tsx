import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  Activity,
  Box,
  Check,
  ChevronDown,
  Command,
  Crosshair,
  Disc3,
  Globe,
  Lock,
  Music2,
  Package,
  Play,
  Search,
  Settings,
  Shield,
  SkipBack,
  SkipForward,
  Sparkles,
  User,
  Waves,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import {
  api,
  type Account,
  type CuratedServer,
  type Instance,
  type JvmPlan,
  type ModHit,
  type PlaytimeSummary,
  type ScanFinding,
  type Schematic,
  type ServerStatus,
  type SystemInfo,
  type SpotifyProfile,
  type VersionInfo,
  type WorldSave,
} from "./api";
import { interpretDj } from "./dj";

type Tab =
  | "library"
  | "mods"
  | "servers"
  | "spotify"
  | "studio"
  | "profiler"
  | "vault"
  | "settings";

type ThemePalette = {
  accent: string;
  accent2: string;
  bg0: string;
  bg1: string;
  text: string;
  muted: string;
};

type WallpaperRecord = { blob: Blob; type: string };

const DEFAULT_PALETTE: ThemePalette = {
  accent: "#6ee7ff",
  accent2: "#a78bfa",
  bg0: "#07080d",
  bg1: "#10131c",
  text: "#eef2ff",
  muted: "#93a0bf",
};

function openSettingsDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("nebula-settings", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("preferences");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function loadWallpaper(): Promise<WallpaperRecord | null> {
  const db = await openSettingsDb();
  return new Promise((resolve, reject) => {
    const request = db.transaction("preferences", "readonly").objectStore("preferences").get("wallpaper");
    request.onsuccess = () => {
      db.close();
      resolve((request.result as WallpaperRecord | undefined) ?? null);
    };
    request.onerror = () => {
      db.close();
      reject(request.error);
    };
  });
}

async function saveWallpaper(file: File): Promise<void> {
  const db = await openSettingsDb();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction("preferences", "readwrite");
    transaction.objectStore("preferences").put({ blob: file, type: file.type } satisfies WallpaperRecord, "wallpaper");
    transaction.oncomplete = () => {
      db.close();
      resolve();
    };
    transaction.onerror = () => {
      db.close();
      reject(transaction.error);
    };
  });
}

async function removeWallpaper(): Promise<void> {
  const db = await openSettingsDb();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction("preferences", "readwrite");
    transaction.objectStore("preferences").delete("wallpaper");
    transaction.oncomplete = () => {
      db.close();
      resolve();
    };
    transaction.onerror = () => {
      db.close();
      reject(transaction.error);
    };
  });
}

type SpotifyTrack = {
  id: string;
  uri: string;
  name: string;
  artists: Array<{ name: string }>;
  album: { images: Array<{ url: string }> };
  duration_ms: number;
};

type SpotifyPlaylist = {
  id: string;
  uri: string;
  name: string;
  images: Array<{ url: string }>;
  tracks: { total: number };
  owner: { display_name?: string; id: string };
};

type SpotifyPlayerState = {
  paused: boolean;
  position: number;
  duration: number;
  track_window: { current_track: SpotifyTrack };
};

type SpotifyPlayer = {
  connect: () => Promise<boolean>;
  disconnect: () => void;
  addListener: (event: string, callback: (payload: unknown) => void) => boolean;
  getCurrentState: () => Promise<SpotifyPlayerState | null>;
  togglePlay: () => Promise<void>;
  nextTrack: () => Promise<void>;
  previousTrack: () => Promise<void>;
};

declare global {
  interface Window {
    Spotify?: { Player: new (options: { name: string; volume: number; getOAuthToken: (callback: (token: string) => void) => void }) => SpotifyPlayer };
    onSpotifyWebPlaybackSDKReady?: () => void;
  }
}

const TABS: { id: Tab; label: string; icon: typeof Box }[] = [
  { id: "library", label: "Library", icon: Box },
  { id: "mods", label: "Mods", icon: Package },
  { id: "servers", label: "Servers", icon: Globe },
  { id: "spotify", label: "Spotify", icon: Music2 },
  { id: "studio", label: "Studio", icon: Sparkles },
  { id: "profiler", label: "Profiler", icon: Activity },
  { id: "vault", label: "Vault", icon: Disc3 },
  { id: "settings", label: "Settings", icon: Settings },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("library");
  const [searchOpen, setSearchOpen] = useState(false);
  const [motionBlur, setMotionBlur] = useState(true);
  const [theme, setTheme] = useState(localStorage.getItem("nebula-theme") || "aurora");
  const [palette, setPalette] = useState<ThemePalette>(() => {
    try {
      return { ...DEFAULT_PALETTE, ...JSON.parse(localStorage.getItem("nebula-palette") || "{}") };
    } catch {
      return DEFAULT_PALETTE;
    }
  });
  const [wallpaper, setWallpaper] = useState<{ url: string; type: string } | null>(null);
  const wallpaperUrl = useRef<string | null>(null);
  const [audio, setAudio] = useState(0.15);
  const [locked, setLocked] = useState(false);
  const [pinConfigured, setPinConfigured] = useState(false);
  const [pinInput, setPinInput] = useState("");
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [toast, setToast] = useState<string>("");
  const [progress, setProgress] = useState<string>("");
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [updateProgress, setUpdateProgress] = useState("");
  const [installingUpdate, setInstallingUpdate] = useState(false);

  useEffect(() => {
    document.documentElement.dataset.theme = theme === "aurora" ? "" : theme;
    for (const key of Object.keys(DEFAULT_PALETTE) as Array<keyof ThemePalette>) {
      if (theme === "custom") document.documentElement.style.setProperty(`--${key}`, palette[key]);
      else document.documentElement.style.removeProperty(`--${key}`);
    }
    localStorage.setItem("nebula-theme", theme);
  }, [theme, palette]);

  useEffect(() => {
    localStorage.setItem("nebula-palette", JSON.stringify(palette));
  }, [palette]);

  useEffect(() => {
    let active = true;
    loadWallpaper().then((record) => {
      if (!active || !record) return;
      const url = URL.createObjectURL(record.blob);
      wallpaperUrl.current = url;
      setWallpaper({ url, type: record.type });
    }).catch(() => null);
    return () => {
      active = false;
      if (wallpaperUrl.current) URL.revokeObjectURL(wallpaperUrl.current);
    };
  }, []);

  useEffect(() => {
    document.body.classList.toggle("motion-blur", motionBlur);
    document.documentElement.dataset.reduceMotion = motionBlur ? "false" : "true";
  }, [motionBlur]);

  useEffect(() => {
    document.documentElement.style.setProperty("--audio", String(audio));
  }, [audio]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSearchOpen(true);
      }
      if (e.key === "Escape") setSearchOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    api.system().then(setSys).catch(() => null);
    api.activeAccount().then(setAccount).catch(() => null);
    api.instances().then((list) => {
      setInstances(list);
      setSelected((id) => id || list[0]?.id || "");
    }).catch(() => null);
    api.hasPin().then((hasPin) => {
      setPinConfigured(hasPin);
      setLocked(hasPin);
    }).catch((error) => notify(`Could not check PIN lock: ${String(error)}`));
    listen<{ stage: string; detail: string; current: number; total: number }>("install-progress", (e) => {
      setProgress(`${e.payload.stage}: ${e.payload.detail} (${e.payload.current}/${e.payload.total})`);
    }).catch(() => null);
  }, []);

  const notify = (msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(""), 4200);
  };

  useEffect(() => {
    if (import.meta.env.VITE_TAURI_UPDATER_ENABLED !== "true") return;
    let active = true;
    check()
      .then((update) => {
        if (active && update) setAvailableUpdate(update);
        else if (update) void update.close();
      })
      .catch((error: unknown) => {
        if (active) notify(`Could not check for launcher updates: ${String(error)}`);
      });
    return () => {
      active = false;
    };
  }, []);

  const installAvailableUpdate = async () => {
    if (!availableUpdate || installingUpdate) return;
    setInstallingUpdate(true);
    setUpdateProgress("Preparing update…");
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setUpdateProgress(event.data.contentLength
            ? `Downloading update (${Math.ceil(event.data.contentLength / 1024 / 1024)} MiB)…`
            : "Downloading update…");
        } else if (event.event === "Progress") {
          setUpdateProgress("Downloading update…");
        } else {
          setUpdateProgress("Installing update…");
        }
      });
      setUpdateProgress("Restarting Nebula…");
      await relaunch();
    } catch (error) {
      setInstallingUpdate(false);
      setUpdateProgress("");
      notify(`Could not install launcher update: ${String(error)}`);
    }
  };

  const refreshInstances = async () => {
    const list = await api.instances();
    setInstances(list);
  };

  const updateWallpaper = async (file: File) => {
    if (file.size > 40 * 1024 * 1024) throw new Error("Wallpaper must be 40 MiB or smaller");
    if (!file.type.startsWith("image/") && file.type !== "video/mp4") throw new Error("Choose an image, GIF, or MP4 video");
    await saveWallpaper(file);
    if (wallpaperUrl.current) URL.revokeObjectURL(wallpaperUrl.current);
    const url = URL.createObjectURL(file);
    wallpaperUrl.current = url;
    setWallpaper({ url, type: file.type });
  };

  const clearWallpaper = async () => {
    await removeWallpaper();
    if (wallpaperUrl.current) URL.revokeObjectURL(wallpaperUrl.current);
    wallpaperUrl.current = null;
    setWallpaper(null);
  };

  if (locked) {
    return (
      <div className="h-full grid place-items-center p-8">
        <div className="glass p-8 w-[380px] text-center">
          <Lock className="mx-auto mb-4" />
          <h1 className="text-2xl font-semibold">Nebula is locked</h1>
          <p className="text-sm mt-2" style={{ color: "var(--muted)" }}>
            Enter your 4-digit PIN. Touch ID / Windows Hello can be wired via the OS keychain next.
          </p>
          <input
            className="mt-5 w-full text-center tracking-[0.6em] text-xl"
            maxLength={4}
            value={pinInput}
            onChange={async (e) => {
              const v = e.target.value.replace(/\D/g, "").slice(0, 4);
              setPinInput(v);
              if (v.length === 4) {
                try {
                  const ok = await api.verifyPin(v);
                  if (ok) {
                    setLocked(false);
                    setPinInput("");
                  } else {
                    setPinInput("");
                    notify("Wrong PIN");
                  }
                } catch (error) {
                  setPinInput("");
                  notify(`Could not verify PIN: ${String(error)}`);
                }
              }
            }}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex relative isolate" style={{ ["--audio" as string]: audio }}>
      {wallpaper && (
        <div className="absolute inset-0 z-0 pointer-events-none overflow-hidden" aria-hidden="true">
          {wallpaper.type === "video/mp4" ? (
            <video className="h-full w-full object-cover opacity-25" src={wallpaper.url} autoPlay muted loop playsInline />
          ) : (
            <div className="h-full w-full bg-cover bg-center opacity-25" style={{ backgroundImage: `url("${wallpaper.url}")` }} />
          )}
        </div>
      )}
      <aside className="relative z-10 w-[232px] p-4 flex flex-col gap-3">
        <div className="px-2 py-3">
          <div className="kicker">Minecraft launcher</div>
          <div className="text-2xl font-semibold tracking-tight">Nebula</div>
        </div>
        <nav className="flex flex-col gap-1">
          {TABS.map((t) => {
            const Icon = t.icon;
            const active = tab === t.id;
            return (
              <button
                key={t.id}
                onClick={() => setTab(t.id)}
                className="flex items-center gap-2 px-3 py-2 rounded-xl text-left"
                style={{
                  background: active ? "color-mix(in oklab, var(--accent) 16%, transparent)" : "transparent",
                  color: active ? "var(--text)" : "var(--muted)",
                }}
              >
                <Icon size={16} />
                {t.label}
              </button>
            );
          })}
        </nav>
        <div className="mt-auto glass p-3 pulse-accent">
          <div className="kicker">Account</div>
          <div className="flex items-center gap-2 mt-1">
            <User size={16} />
            <div className="truncate text-sm">{account?.name ?? "Not signed in"}</div>
          </div>
          <div className="text-xs mt-1" style={{ color: "var(--muted)" }}>
            {sys ? `${sys.arch} · ${Math.round(sys.total_ram_mb / 1024)} GB` : "…"}
          </div>
          {pinConfigured && (
            <button
              className="mt-3 flex w-full items-center justify-center gap-2 rounded-lg px-3 py-2 text-sm"
              style={{ border: "1px solid var(--stroke)" }}
              onClick={() => {
                setPinInput("");
                setLocked(true);
              }}
            >
              <Lock size={14} /> Lock Nebula
            </button>
          )}
        </div>
      </aside>

      <main className="relative z-10 flex-1 p-4 pl-0 min-w-0 flex flex-col gap-3">
        <header className="glass min-w-0 px-4 py-3 flex items-center gap-3 pulse-accent">
          <button className="flex min-w-0 flex-1 items-center gap-2 text-sm text-left" style={{ color: "var(--muted)" }} onClick={() => setSearchOpen(true)}>
            <Search size={16} className="shrink-0" />
            <span className="min-w-0 flex-1 truncate">Search versions, mods, servers, tracks…</span>
            <span className="ml-auto shrink-0 px-2 py-0.5 rounded-md text-xs" style={{ border: "1px solid var(--stroke)" }}>
              <Command size={10} className="inline" /> K
            </span>
          </button>
          <div className="ml-auto flex shrink-0 items-center gap-3 text-sm">
            <label className="flex shrink-0 items-center gap-2" style={{ color: "var(--muted)" }}>
              <Waves size={14} /> Motion blur
              <input type="checkbox" checked={motionBlur} onChange={(e) => setMotionBlur(e.target.checked)} />
            </label>
            {progress && <span className="text-xs truncate max-w-[280px]">{progress}</span>}
          </div>
        </header>

        {availableUpdate && (
          <div className="glass flex flex-wrap items-center gap-3 px-4 py-3" role="status">
            <div className="min-w-0 flex-1">
              <div className="font-medium">Nebula update available · {availableUpdate.version}</div>
              <div className="text-sm mt-1 whitespace-pre-line" style={{ color: "var(--muted)" }}>
                {updateProgress || availableUpdate.body || `Version ${availableUpdate.version} is ready to install.`}
              </div>
            </div>
            <button
              className="px-4 py-2 rounded-xl text-sm font-medium"
              style={{ background: "var(--accent)", color: "#081018" }}
              disabled={installingUpdate}
              onClick={() => void installAvailableUpdate()}
            >
              {installingUpdate ? "Updating…" : "Restart and Update"}
            </button>
          </div>
        )}

        <section className="glass flex-1 min-h-0 overflow-auto p-5">
          {tab === "library" && (
            <Library
              instances={instances}
              selected={selected}
              onSelect={setSelected}
              onRefresh={refreshInstances}
              notify={notify}
              setProgress={setProgress}
            />
          )}
          {tab === "mods" && <Mods instance={instances.find((item) => item.id === selected) ?? null} notify={notify} />}
          {tab === "servers" && <Servers instanceId={selected} notify={notify} />}
          {tab === "spotify" && <Spotify onEnergy={setAudio} notify={notify} />}
          {tab === "studio" && <Studio instanceId={selected} notify={notify} />}
          {tab === "profiler" && <Profiler instanceId={selected} sys={sys} />}
          {tab === "vault" && <Vault instanceId={selected} />}
          {tab === "settings" && (
            <SettingsView
              theme={theme}
              setTheme={setTheme}
              palette={palette}
              setPalette={setPalette}
              onWallpaper={updateWallpaper}
              onClearWallpaper={clearWallpaper}
              hasWallpaper={wallpaper !== null}
              motionBlur={motionBlur}
              setMotionBlur={setMotionBlur}
              pinConfigured={pinConfigured}
              setPinConfigured={setPinConfigured}
              setLocked={setLocked}
              setPinInput={setPinInput}
              account={account}
              setAccount={setAccount}
              notify={notify}
              sys={sys}
            />
          )}
        </section>
      </main>

      {searchOpen && (
        <CommandPalette
          instances={instances}
          onClose={() => setSearchOpen(false)}
          onTab={(t) => {
            setTab(t);
            setSearchOpen(false);
          }}
          onInstance={(id) => {
            setSelected(id);
            setTab("library");
            setSearchOpen(false);
          }}
          onTheme={(nextTheme) => {
            setTheme(nextTheme);
            setSearchOpen(false);
          }}
          onMotionBlur={(enabled) => {
            setMotionBlur(enabled);
            setSearchOpen(false);
          }}
          theme={theme}
          motionBlur={motionBlur}
        />
      )}
      {toast && (
        <div className="fixed bottom-5 right-5 glass px-4 py-3 text-sm max-w-sm">{toast}</div>
      )}
    </div>
  );
}

function Library({
  instances,
  selected,
  onSelect,
  onRefresh,
  notify,
  setProgress,
}: {
  instances: Instance[];
  selected: string;
  onSelect: (id: string) => void;
  onRefresh: () => Promise<void>;
  notify: (s: string) => void;
  setProgress: (s: string) => void;
}) {
  const [versions, setVersions] = useState<VersionInfo[]>([]);
  const [name, setName] = useState("New instance");
  const [ver, setVer] = useState("1.21.1");
  const [loader, setLoader] = useState("fabric");
  const [plan, setPlan] = useState<JvmPlan | null>(null);
  const current = instances.find((i) => i.id === selected);

  useEffect(() => {
    api.versions().then((m) => setVersions(m.versions)).catch(() => null);
  }, []);
  useEffect(() => {
    if (current) api.planJvm(current.minecraftVersion).then(setPlan).catch(() => null);
  }, [current?.minecraftVersion]);

  return (
    <div className="grid grid-cols-[280px_1fr] gap-5 min-h-full">
      <div>
        <div className="kicker">Instances</div>
        <div className="mt-3 flex flex-col gap-2">
          {instances.map((i) => (
            <button
              key={i.id}
              onClick={() => onSelect(i.id)}
              className="text-left p-3 rounded-2xl"
              style={{
                background: i.id === selected ? "color-mix(in oklab, var(--accent) 14%, transparent)" : "rgba(255,255,255,0.03)",
                border: "1px solid var(--stroke)",
              }}
            >
              <div className="font-medium">{i.name}</div>
              <div className="text-xs" style={{ color: "var(--muted)" }}>
                {i.minecraftVersion} · {i.loader} · {i.profile}
              </div>
            </button>
          ))}
        </div>
        <div className="instance-create mt-5 space-y-3">
          <div className="flex items-center justify-between px-1">
            <div className="kicker">Create instance</div>
            <span className="text-xs" style={{ color: "var(--muted)" }}>New profile</span>
          </div>
          <input className="w-full" value={name} onChange={(e) => setName(e.target.value)} placeholder="Instance name" aria-label="Instance name" />
          <GlassSelect
            label="Minecraft version"
            icon={Disc3}
            value={ver}
            onChange={setVer}
            searchable
            options={versions.map((version) => ({ value: version.id, label: `${version.id} · ${version.type}` }))}
          />
          <GlassSelect
            label="Mod loader"
            icon={Package}
            value={loader}
            onChange={setLoader}
            options={[
              { value: "fabric", label: "Fabric" },
              { value: "forge", label: "Forge" },
              { value: "neoforge", label: "NeoForge" },
              { value: "quilt", label: "Quilt" },
              { value: "vanilla", label: "Vanilla" },
            ]}
          />
          <button
            className="accent-button w-full py-2.5 rounded-xl font-medium"
            onClick={async () => {
              await api.upsertInstance({
                id: "",
                name,
                minecraftVersion: ver,
                loader,
                profile: loader === "fabric" ? "sodium-iris" : "balanced",
              });
              await onRefresh();
              notify("Instance created");
            }}
          >
            Add instance
          </button>
        </div>
      </div>
      <div>
        {current ? (
          <>
            <h2 className="text-3xl font-semibold">{current.name}</h2>
            <p className="mt-1" style={{ color: "var(--muted)" }}>
              Official Microsoft account required to launch. No offline / cracked mode.
            </p>
            {plan && (
              <div className="mt-4 grid grid-cols-3 gap-3">
                <Stat label="Java" value={plan.matched ? `${plan.matched.major}` : `need ${plan.required_major}+`} />
                <Stat label="Heap" value={`${plan.heap_mb} MB`} />
                <Stat label="GC" value={plan.required_major >= 21 ? "ZGC" : "G1 / Aikar"} />
              </div>
            )}
            <p className="text-sm mt-3" style={{ color: "var(--muted)" }}>{plan?.rationale}</p>
            <div className="flex gap-3 mt-6">
              <button
                className="px-5 py-3 rounded-2xl font-medium"
                style={{ background: "var(--accent)", color: "#081018" }}
                onClick={async () => {
                  try {
                    setProgress("Installing / launching…");
                    const r = await api.launch(current.id);
                    notify(`Launched pid ${r.pid}`);
                  } catch (e) {
                    notify(String(e));
                  }
                }}
                disabled={!(["vanilla", "fabric", "quilt", "forge", "neoforge"].includes(current.loader))}
              >
                Play
              </button>
              <button
                className="px-5 py-3 rounded-2xl"
                style={{ border: "1px solid var(--stroke)" }}
                onClick={async () => {
                  try {
                    await api.installInstance(current.id);
                    await onRefresh();
                    notify(`${current.loader} files installed`);
                  } catch (e) {
                    notify(String(e));
                  }
                }}
              >
                Install files
              </button>
            </div>
          </>
        ) : (
          <p>Select an instance.</p>
        )}
      </div>
    </div>
  );
}

function GlassSelect({
  label,
  icon: Icon,
  value,
  options,
  onChange,
  searchable = false,
}: {
  label: string;
  icon: typeof Search;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
  searchable?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const listId = useId();
  const filtered = options.filter((option) => option.label.toLowerCase().includes(query.trim().toLowerCase()));
  const selected = options.find((option) => option.value === value);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    const focusTimer = window.setTimeout(() => {
      if (searchable) searchRef.current?.focus();
      else optionRefs.current[activeIndex]?.focus();
    }, 0);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.clearTimeout(focusTimer);
    };
  }, [open, searchable, activeIndex]);

  useEffect(() => {
    setActiveIndex(0);
    optionRefs.current = [];
  }, [query]);

  const moveActive = (direction: number) => {
    if (!filtered.length) return;
    const next = (activeIndex + direction + filtered.length) % filtered.length;
    setActiveIndex(next);
    optionRefs.current[next]?.focus();
  };

  const handleOptionKey = (event: ReactKeyboardEvent<HTMLButtonElement>, index: number) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      const next = event.key === "Home" ? 0 : filtered.length - 1;
      setActiveIndex(next);
      optionRefs.current[next]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      triggerRef.current?.focus();
    } else if (event.key === "Tab") {
      setOpen(false);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onChange(filtered[index].value);
      setOpen(false);
      setQuery("");
      triggerRef.current?.focus();
    }
  };

  return (
    <div className="glass-select" ref={rootRef}>
      <button
        ref={triggerRef}
        type="button"
        className="glass-select-trigger"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <Icon size={16} className="shrink-0" />
        <span className="min-w-0 flex-1 text-left">
          <span className="glass-select-label">{label}</span>
          <span className="glass-select-value">{selected?.label ?? value}</span>
        </span>
        <ChevronDown size={16} className={`glass-select-chevron ${open ? "is-open" : ""}`} />
      </button>
      <div className={`glass-select-popover ${open ? "is-open" : ""}`} aria-hidden={!open}>
        {searchable && (
          <div className="glass-select-search">
            <Search size={15} />
            <input
              ref={searchRef}
              value={query}
              aria-label={`Search ${label.toLowerCase()}s`}
              placeholder={`Search ${label.toLowerCase()}s…`}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown" && filtered.length) {
                  event.preventDefault();
                  setActiveIndex(0);
                  optionRefs.current[0]?.focus();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  setOpen(false);
                  triggerRef.current?.focus();
                }
              }}
            />
          </div>
        )}
        <div className="glass-select-options" id={listId} role="listbox" aria-label={label}>
          {filtered.slice(0, 120).map((option, index) => (
            <button
              ref={(element) => { optionRefs.current[index] = element; }}
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              tabIndex={open && index === activeIndex ? 0 : -1}
              className="glass-select-option"
              onFocus={() => setActiveIndex(index)}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
                setQuery("");
                triggerRef.current?.focus();
              }}
              onKeyDown={(event) => handleOptionKey(event, index)}
            >
              <span className="truncate">{option.label}</span>
              {option.value === value && <Check size={15} className="shrink-0" />}
            </button>
          ))}
          {filtered.length === 0 && <div className="glass-select-empty">No matching versions</div>}
          {filtered.length > 120 && <div className="glass-select-empty">Showing 120 matches. Refine your search.</div>}
        </div>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="p-4 rounded-2xl" style={{ background: "rgba(255,255,255,0.03)", border: "1px solid var(--stroke)" }}>
      <div className="kicker">{label}</div>
      <div className="text-xl mt-1">{value}</div>
    </div>
  );
}

const MOD_CATALOG: Array<{ slug: string; name: string; loaders: string[] }> = [
  { slug: "fabric-api", name: "Fabric API", loaders: ["fabric", "quilt"] },
  { slug: "sodium", name: "Sodium", loaders: ["fabric", "quilt"] },
  { slug: "lithium", name: "Lithium", loaders: ["fabric", "quilt"] },
  { slug: "iris", name: "Iris Shaders", loaders: ["fabric", "quilt"] },
  { slug: "sodium-extra", name: "Sodium Extra", loaders: ["fabric", "quilt"] },
  { slug: "reeses-sodium-options", name: "Reese's Sodium Options", loaders: ["fabric", "quilt"] },
  { slug: "modmenu", name: "Mod Menu", loaders: ["fabric", "quilt"] },
  { slug: "continuity", name: "Continuity", loaders: ["fabric", "quilt"] },
  { slug: "roughly-enough-items", name: "Roughly Enough Items", loaders: ["fabric", "quilt"] },
  { slug: "embeddium", name: "Embeddium", loaders: ["forge", "neoforge"] },
  { slug: "oculus", name: "Oculus Shaders", loaders: ["forge", "neoforge"] },
  { slug: "jei", name: "Just Enough Items", loaders: ["forge", "neoforge"] },
  { slug: "configured", name: "Configured", loaders: ["forge", "neoforge"] },
  { slug: "controlling", name: "Controlling", loaders: ["forge", "neoforge"] },
  { slug: "ferrite-core", name: "FerriteCore", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "entityculling", name: "Entity Culling", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "immediatelyfast", name: "ImmediatelyFast", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "modernfix", name: "ModernFix", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "dynamic-fps", name: "Dynamic FPS", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "appleskin", name: "AppleSkin", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "jade", name: "Jade", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "xaeros-minimap", name: "Xaero's Minimap", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "xaeros-world-map", name: "Xaero's World Map", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "mouse-tweaks", name: "Mouse Tweaks", loaders: ["fabric", "quilt", "forge", "neoforge"] },
  { slug: "betterf3", name: "BetterF3", loaders: ["fabric", "quilt", "forge", "neoforge"] },
];

function Mods({ instance, notify }: { instance: Instance | null; notify: (message: string) => void }) {
  const [q, setQ] = useState("sodium");
  const [source, setSource] = useState<"modrinth" | "curseforge">("modrinth");
  const [curseForgeKey, setCurseForgeKey] = useState("");
  const [hits, setHits] = useState<ModHit[]>([]);
  const [scan, setScan] = useState<ScanFinding[]>([]);
  const [installing, setInstalling] = useState("");
  const [selectedMods, setSelectedMods] = useState<string[]>(MOD_CATALOG.map((mod) => mod.slug));
  const [batchProgress, setBatchProgress] = useState("");
  const compatibleMods = MOD_CATALOG.filter((mod) => instance && mod.loaders.includes(instance.loader));
  const search = async () => {
    if (source === "curseforge") {
      if (!instance) throw new Error("Select a modded instance first");
      if (!curseForgeKey.trim()) throw new Error("Enter a CurseForge API key");
      const results = await api.searchCurseForgeMods(q, instance.minecraftVersion, instance.loader, curseForgeKey.trim());
      setHits(results.map((hit) => ({
        project_id: String(hit.id),
        slug: hit.slug,
        title: hit.name,
        description: hit.summary,
        downloads: hit.downloadCount,
        icon_url: hit.logoUrl ?? undefined,
      })));
      return;
    }
    const res = await api.searchMods(q, JSON.stringify([["project_type:mod"]]));
    setHits(res.hits ?? []);
  };
  useEffect(() => {
    search().catch(() => null);
  }, []);

  const installSelectedMods = async () => {
    if (!instance || installing) return;
    const selected = compatibleMods.filter((mod) => selectedMods.includes(mod.slug));
    if (selected.length === 0) {
      notify("Select at least one mod");
      return;
    }
    setInstalling("batch");
    const installed: string[] = [];
    const skipped: string[] = [];
    const failed: string[] = [];
    try {
      for (let index = 0; index < selected.length; index += 1) {
        const mod = selected[index];
        setBatchProgress(`Installing ${index + 1}/${selected.length}: ${mod.name}`);
        try {
          const versions = await api.modVersions(mod.slug, instance.minecraftVersion, instance.loader);
          const version = versions[0];
          if (!version) {
            skipped.push(mod.name);
            continue;
          }
          await api.downloadMod(instance.id, version.id);
          installed.push(mod.name);
        } catch (error) {
          failed.push(`${mod.name}: ${String(error)}`);
        }
      }
      const findings = await api.scanMods(instance.id);
      setScan(findings);
      const flagged = findings.filter((finding) => finding.blocked).length;
      notify(`${installed.length} installed, ${skipped.length} unavailable, ${failed.length} failed${flagged ? `; ${flagged} flagged by heuristic scan` : ""}`);
      if (failed.length) setBatchProgress(failed.slice(0, 3).join("\n"));
      else if (skipped.length) setBatchProgress(`No compatible build: ${skipped.join(", ")}`);
      else setBatchProgress("");
    } catch (error) {
      notify(String(error));
    } finally {
      setInstalling("");
    }
  };

  return (
    <div>
      <div className="mb-5 p-4 rounded-xl" style={{ border: "1px solid var(--stroke)" }}>
        <div className="flex flex-wrap items-start gap-3">
          <div className="flex-1 min-w-48">
            <div className="font-medium">Curated mod library</div>
            <div className="text-xs mt-1" style={{ color: "var(--muted)" }}>
              {compatibleMods.length} mods matched to {instance ? `${instance.minecraftVersion} ${instance.loader}` : "the selected instance"}. Required dependencies install automatically.
            </div>
          </div>
          <button className="px-4 py-2 rounded-xl" style={{ background: "var(--accent)", color: "#081018" }} disabled={!instance || !compatibleMods.length || !!installing} onClick={() => void installSelectedMods()}>
            {installing === "batch" ? "Installing…" : `Install ${selectedMods.filter((slug) => compatibleMods.some((mod) => mod.slug === slug)).length} selected`}
          </button>
        </div>
        <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-x-4 gap-y-2 mt-4">
          {compatibleMods.map((mod) => (
            <label key={mod.slug} className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={selectedMods.includes(mod.slug)} disabled={!!installing} onChange={(event) => {
                setSelectedMods((current) => event.target.checked
                  ? [...current, mod.slug]
                  : current.filter((slug) => slug !== mod.slug));
              }} />
              {mod.name}
            </label>
          ))}
        </div>
        {batchProgress && <p className="text-xs mt-3 whitespace-pre-wrap" style={{ color: "var(--muted)" }}>{batchProgress}</p>}
      </div>
      <div className="flex gap-2">
        <input className="flex-1" value={q} onChange={(e) => setQ(e.target.value)} placeholder={`Search ${source === "modrinth" ? "Modrinth" : "CurseForge"}`} />
        <select className="max-w-40" value={source} onChange={(event) => {
          setSource(event.target.value as "modrinth" | "curseforge");
          setHits([]);
        }}>
          <option value="modrinth">Modrinth</option>
          <option value="curseforge">CurseForge</option>
        </select>
        <button className="px-4 rounded-xl" style={{ background: "var(--accent)", color: "#081018" }} onClick={() => search().catch((error) => notify(String(error)))}>
          Search
        </button>
        <button
          className="px-4 rounded-xl flex items-center gap-2"
          style={{ border: "1px solid var(--stroke)" }}
          disabled={!instance}
          onClick={async () => {
            if (!instance) return;
            setScan(await api.scanMods(instance.id));
          }}
        >
          <Shield size={14} /> Scan jars
        </button>
      </div>
      {source === "curseforge" && (
        <input className="w-full mt-3" type="password" autoComplete="off" placeholder="CurseForge API key" value={curseForgeKey} onChange={(event) => setCurseForgeKey(event.target.value)} />
      )}
      <div className="mt-4 grid gap-2">
        {hits.map((h) => (
          <div key={h.project_id} className="flex items-center gap-3 p-3 rounded-2xl" style={{ border: "1px solid var(--stroke)" }}>
            {h.icon_url && <img src={h.icon_url} className="w-10 h-10 rounded-lg" alt="" />}
            <div className="min-w-0 flex-1">
              <div className="font-medium">{h.title}</div>
              <div className="text-xs truncate" style={{ color: "var(--muted)" }}>{h.description}</div>
            </div>
            <div className="text-xs" style={{ color: "var(--muted)" }}>{h.downloads.toLocaleString()} dl</div>
            <button
              className="px-3 py-2 rounded-xl text-sm"
              style={{ background: "var(--accent)", color: "#081018" }}
              disabled={!instance || installing !== ""}
              onClick={async () => {
                if (!instance) return;
                setInstalling(h.project_id);
                try {
                  let installedPath: string;
                  let installedVersion = "";
                  if (source === "curseforge") {
                    if (!curseForgeKey.trim()) throw new Error("Enter a CurseForge API key");
                    const files = await api.curseForgeFiles(Number(h.project_id), instance.minecraftVersion, instance.loader, curseForgeKey.trim());
                    const file = files[0];
                    if (!file) {
                      notify(`No ${instance.loader} file for Minecraft ${instance.minecraftVersion}`);
                      return;
                    }
                    installedVersion = file.fileName;
                    installedPath = await api.installCurseForgeFile(instance.id, file.id, curseForgeKey.trim());
                  } else {
                    const versions = await api.modVersions(h.project_id, instance.minecraftVersion, instance.loader);
                    const version = versions[0];
                    if (!version) {
                      notify(`No ${instance.loader} build for Minecraft ${instance.minecraftVersion}`);
                      return;
                    }
                    installedVersion = version.version_number;
                    installedPath = await api.downloadMod(instance.id, version.id);
                  }
                  const findings = await api.scanMods(instance.id);
                  setScan(findings);
                  const installed = findings.find((finding) => finding.path === installedPath);
                  notify(installed?.blocked
                    ? `${h.title} installed but flagged by the heuristic scan`
                    : `${h.title} ${installedVersion} installed and scanned`);
                } catch (error) {
                  notify(String(error));
                } finally {
                  setInstalling("");
                }
              }}
            >
              {installing === h.project_id ? "Installing…" : "Install"}
            </button>
            <a className="text-sm" href={`https://modrinth.com/mod/${h.slug}`} target="_blank" rel="noreferrer">
              Open
            </a>
          </div>
        ))}
      </div>
      {scan.length > 0 && (
        <div className="mt-6">
          <div className="kicker">Heuristic JAR scan, not antivirus</div>
          {scan.map((s) => (
            <div key={s.path} className="text-sm mt-2">
              <span style={{ color: s.blocked ? "var(--danger)" : "var(--ok)" }}>
                {s.blocked ? "Flagged" : "No matches"}
              </span>{" "}
              {s.path.split(/[/\\]/).pop()} {s.reasons.join("; ")}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function serverKey(server: CuratedServer) {
  return `${server.host}:${server.port}`;
}

function Servers({ instanceId, notify }: { instanceId: string; notify: (s: string) => void }) {
  const [list, setList] = useState<CuratedServer[]>([]);
  const [status, setStatus] = useState<Record<string, ServerStatus>>({});
  const [query, setQuery] = useState("");
  const [versionFilter, setVersionFilter] = useState("");
  const [modeFilter, setModeFilter] = useState("");
  const [regionFilter, setRegionFilter] = useState("");
  const [page, setPage] = useState(1);
  const [refreshKey, setRefreshKey] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");

  useEffect(() => {
    let active = true;
    const loadServers = async () => {
      setLoading(true);
      setLoadError("");
      try {
        const servers = await api.servers();
        if (!active) return;
        setList(servers);
        const results = await Promise.all(servers.map(async (server) => {
          try {
            return [serverKey(server), await api.ping(server.host, server.port)] as const;
          } catch (error) {
            return [serverKey(server), {
              host: server.host,
              port: server.port,
              online: false,
              error: String(error),
            } satisfies ServerStatus] as const;
          }
        }));
        if (active) setStatus(Object.fromEntries(results));
      } catch (error) {
        if (active) setLoadError(String(error));
      } finally {
        if (active) setLoading(false);
      }
    };
    void loadServers();
    return () => {
      active = false;
    };
  }, [refreshKey]);

  const modes = useMemo(() => Array.from(new Set(list.flatMap((server) => server.tags))).sort(), [list]);
  const regions = useMemo(() => Array.from(new Set(list.map((server) => server.region))).sort(), [list]);
  const filteredServers = useMemo(() => {
    const searchTerm = query.trim().toLowerCase();
    return list.filter((server) => {
      const currentStatus = status[serverKey(server)];
      const searchable = [
        server.name,
        server.host,
        server.region,
        ...server.tags,
        currentStatus?.motd ?? "",
        currentStatus?.version ?? "",
      ].join(" ").toLowerCase();
      return (!searchTerm || searchable.includes(searchTerm))
        && (!versionFilter || currentStatus?.version?.includes(versionFilter))
        && (!modeFilter || server.tags.includes(modeFilter))
        && (!regionFilter || server.region === regionFilter);
    });
  }, [list, modeFilter, query, regionFilter, status, versionFilter]);
  const visibleServers = filteredServers.slice(0, page * 8);

  useEffect(() => {
    setPage(1);
  }, [query, versionFilter, modeFilter, regionFilter]);

  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-2xl font-semibold">Public servers</h2>
          <p className="text-sm mt-1" style={{ color: "var(--muted)" }}>
            Curated Java networks with live Server List Ping. Authentication mode is not reported by status services.
          </p>
        </div>
        <button className="px-3 py-2 rounded-xl text-sm" style={{ border: "1px solid var(--stroke)" }} disabled={loading} onClick={() => setRefreshKey((key) => key + 1)}>
          {loading ? "Refreshing…" : "Refresh"}
        </button>
      </div>
      <div className="mt-4 grid sm:grid-cols-2 lg:grid-cols-4 gap-2">
        <input className="sm:col-span-2" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search name, address, mode, or MOTD" />
        <select value={versionFilter} onChange={(event) => setVersionFilter(event.target.value)} aria-label="Filter by Minecraft version">
          <option value="">Any version</option>
          <option value="1.8">1.8.x</option>
          <option value="1.20">1.20.x</option>
          <option value="1.21">1.21.x</option>
        </select>
        <select value={modeFilter} onChange={(event) => setModeFilter(event.target.value)} aria-label="Filter by game mode">
          <option value="">Any game mode</option>
          {modes.map((mode) => <option key={mode} value={mode}>{mode}</option>)}
        </select>
        <select value={regionFilter} onChange={(event) => setRegionFilter(event.target.value)} aria-label="Filter by region">
          <option value="">Any region</option>
          {regions.map((region) => <option key={region} value={region}>{region}</option>)}
        </select>
      </div>
      {loadError && <p className="mt-3 text-sm" style={{ color: "var(--danger, #f87171)" }}>Could not load servers: {loadError}</p>}
      {!loading && !loadError && filteredServers.length === 0 && (
        <p className="mt-4 text-sm" style={{ color: "var(--muted)" }}>No servers match these filters.</p>
      )}
      <div className="mt-4 grid gap-2">
        {visibleServers.map((server) => {
          const currentStatus = status[serverKey(server)];
          return (
            <div key={serverKey(server)} className="flex flex-wrap items-center gap-4 p-4 rounded-2xl" style={{ border: "1px solid var(--stroke)", background: "rgba(255,255,255,0.025)" }}>
              {currentStatus?.favicon && (
                <img className="h-11 w-11 rounded-lg" src={currentStatus.favicon} alt="" />
              )}
              <div className="min-w-48 flex-1">
                <div className="font-medium">{server.name}</div>
                <div className="text-xs mt-1" style={{ color: "var(--muted)" }}>
                  {server.host}{server.port !== 25565 ? `:${server.port}` : ""} · {currentStatus?.motd || server.tags.join(" · ")}
                </div>
                <div className="flex flex-wrap gap-1 mt-2">
                  {server.tags.map((tag) => (
                    <span key={tag} className="text-xs px-2 py-0.5 rounded-full" style={{ background: "rgba(255,255,255,0.06)", color: "var(--muted)" }}>{tag}</span>
                  ))}
                  <span className="text-xs px-2 py-0.5">{server.region}</span>
                </div>
              </div>
              <div className="flex items-center gap-2 text-sm min-w-24">
                <span className="inline-block h-2.5 w-2.5 rounded-full" style={{ background: currentStatus?.online ? "#4ade80" : currentStatus ? "#f87171" : "#94a3b8" }} />
                <span>{currentStatus?.online ? "Online" : currentStatus ? "Offline" : "Checking"}</span>
              </div>
              <div className="text-sm min-w-24">
                {currentStatus?.online ? `${currentStatus.playersOnline ?? "?"} / ${currentStatus.playersMax ?? "?"} players` : "—"}
              </div>
              <div className="text-sm min-w-16" style={{ color: "var(--muted)" }}>
                {currentStatus?.pingMs != null ? `${currentStatus.pingMs} ms` : "—"}
              </div>
              <button
                className="px-3 py-2 rounded-xl text-sm"
                style={{ background: "var(--accent)", color: "#081018" }}
                onClick={async () => {
                  try {
                    await api.launch(instanceId, server.host, server.port);
                    notify(`Connecting to ${server.name}`);
                  } catch (error) {
                    notify(String(error));
                  }
                }}
              >
                Quick Connect
              </button>
            </div>
          );
        })}
      </div>
      {visibleServers.length < filteredServers.length && (
        <button className="mt-4 px-4 py-2 rounded-xl" style={{ border: "1px solid var(--stroke)" }} onClick={() => setPage((current) => current + 1)}>
          Load more ({filteredServers.length - visibleServers.length} remaining)
        </button>
      )}
    </div>
  );
}

function Spotify({ onEnergy, notify }: { onEnergy: (n: number) => void; notify: (message: string) => void }) {
  const notifyRef = useRef(notify);
  const configuredClientId = import.meta.env.VITE_SPOTIFY_CLIENT_ID?.trim()
    || localStorage.getItem("spotify-client-id")?.trim()
    || "";
  const [prompt, setPrompt] = useState("Play chill lofi beats for building");
  const [plan, setPlan] = useState(interpretDj(prompt));
  const [token, setToken] = useState("");
  const [spotifyProfile, setSpotifyProfile] = useState<SpotifyProfile | null>(null);
  const [playlists, setPlaylists] = useState<SpotifyPlaylist[]>([]);
  const [selectedPlaylist, setSelectedPlaylist] = useState<SpotifyPlaylist | null>(null);
  const [deviceId, setDeviceId] = useState("");
  const [tracks, setTracks] = useState<SpotifyTrack[]>([]);
  const [playback, setPlayback] = useState<SpotifyPlayerState | null>(null);
  const [busy, setBusy] = useState(false);
  const playerRef = useRef<SpotifyPlayer | null>(null);

  useEffect(() => {
    notifyRef.current = notify;
  }, [notify]);

  useEffect(() => {
    api.spotifyCurrentUser().then(setSpotifyProfile).then(() => api.spotifyAccessToken()).then(setToken).catch(() => null);
    let unlisten: (() => void) | undefined;
    listen<{ success: boolean; detail: string }>("spotify-auth-status", (event) => {
      notifyRef.current(event.payload.detail);
      if (event.payload.success) {
        api.spotifyCurrentUser()
          .then((profile) => {
            setSpotifyProfile(profile);
            return api.spotifyAccessToken();
          })
          .then(setToken)
          .catch((error) => notifyRef.current(String(error)));
      }
    }).then((stop) => {
      unlisten = stop;
    }).catch(() => null);
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!token) return;
    let active = true;
    const initialize = () => {
      if (!active || !window.Spotify || playerRef.current) return;
      const player = new window.Spotify.Player({
        name: "Nebula Launcher",
        volume: 0.7,
        getOAuthToken: (callback) => {
          api.spotifyAccessToken().then((freshToken) => {
            if (active) {
              setToken(freshToken);
              callback(freshToken);
            }
          }).catch((error) => notifyRef.current(String(error)));
        },
      });
      player.addListener("ready", (payload) => {
        const id = (payload as { device_id?: string } | null)?.device_id;
        if (id) setDeviceId(id);
      });
      player.addListener("not_ready", () => setDeviceId(""));
      player.addListener("player_state_changed", (payload) => setPlayback(payload as SpotifyPlayerState | null));
      player.addListener("initialization_error", (payload) => notifyRef.current((payload as { message?: string } | null)?.message ?? "Spotify player initialization failed"));
      player.addListener("authentication_error", (payload) => notifyRef.current((payload as { message?: string } | null)?.message ?? "Spotify authentication failed"));
      player.addListener("account_error", () => notifyRef.current("Spotify Web Playback requires Premium"));
      playerRef.current = player;
      void player.connect();
    };

    window.onSpotifyWebPlaybackSDKReady = initialize;
    let script = document.getElementById("spotify-playback-sdk") as HTMLScriptElement | null;
    if (!script) {
      script = document.createElement("script");
      script.id = "spotify-playback-sdk";
      script.src = "https://sdk.scdn.co/spotify-player.js";
      script.async = true;
      document.body.appendChild(script);
    }
    if (window.Spotify) initialize();

    return () => {
      active = false;
      if (playerRef.current) {
        playerRef.current.disconnect();
        playerRef.current = null;
      }
      setDeviceId("");
    };
  }, [token]);

  const spotifyRequest = async <T,>(endpoint: string, init: RequestInit = {}): Promise<T> => {
    const accessToken = await api.spotifyAccessToken();
    setToken(accessToken);
    const response = await fetch(`https://api.spotify.com/v1${endpoint}`, {
      ...init,
      headers: { Authorization: `Bearer ${accessToken}`, "Content-Type": "application/json", ...init.headers },
    });
    if (!response.ok) throw new Error(`Spotify API ${response.status}: ${await response.text()}`);
    return response.status === 204 ? (undefined as T) : response.json() as Promise<T>;
  };

  useEffect(() => {
    if (!token) {
      setPlaylists([]);
      setSelectedPlaylist(null);
      return;
    }
    let active = true;
    spotifyRequest<{ items: SpotifyPlaylist[]; next: string | null }>("/me/playlists?limit=50")
      .then((result) => {
        if (active) setPlaylists(result.items);
      })
      .catch((error) => notifyRef.current(`Could not load Spotify playlists: ${String(error)}`));
    return () => {
      active = false;
    };
  }, [token]);

  const openPlaylist = async (playlist: SpotifyPlaylist) => {
    setSelectedPlaylist(playlist);
    setBusy(true);
    try {
      const result = await spotifyRequest<{ items: Array<{ item?: SpotifyTrack | null; track?: SpotifyTrack | null }> }>(
        `/playlists/${encodeURIComponent(playlist.id)}/items?limit=50`,
      );
      setTracks(result.items.map((entry) => entry.item ?? entry.track).filter((track): track is SpotifyTrack => !!track?.uri));
    } catch (error) {
      notify(String(error));
    } finally {
      setBusy(false);
    }
  };

  const searchTracks = async (query: string) => {
    const result = await spotifyRequest<{ tracks: { items: SpotifyTrack[] } }>(`/search?type=track&limit=8&q=${encodeURIComponent(query)}`);
    setTracks(result.tracks.items);
    return result.tracks.items;
  };

  const playTrack = async (track: SpotifyTrack) => {
    if (!deviceId) throw new Error("Spotify player is still connecting. Wait for the device to appear.");
    await spotifyRequest("/me/player", { method: "PUT", body: JSON.stringify({ device_ids: [deviceId], play: true }) });
    await spotifyRequest(`/me/player/play?device_id=${encodeURIComponent(deviceId)}`, {
      method: "PUT",
      body: JSON.stringify({ uris: [track.uri] }),
    });
    setPlayback(await playerRef.current?.getCurrentState() ?? null);
  };

  const playPlaylist = async (playlist: SpotifyPlaylist) => {
    if (!deviceId) {
      notify("Spotify player is still connecting. Wait for the device to appear.");
      return;
    }
    try {
      await spotifyRequest("/me/player", { method: "PUT", body: JSON.stringify({ device_ids: [deviceId], play: true }) });
      await spotifyRequest(`/me/player/play?device_id=${encodeURIComponent(deviceId)}`, {
        method: "PUT",
        body: JSON.stringify({ context_uri: playlist.uri }),
      });
    } catch (error) {
      notify(String(error));
    }
  };

  const connectSpotify = async () => {
    if (!configuredClientId) {
      notify("Spotify sign-in isn’t configured for this build. The app publisher must set VITE_SPOTIFY_CLIENT_ID and register the Nebula callback URI.");
      return;
    }
    try {
      const url = await api.spotifyAuthorizeUrl(configuredClientId);
      await openUrl(url);
      notify("Complete Spotify sign-in in your browser");
    } catch (error) {
      notify(String(error));
    }
  };

  const playDjPick = async () => {
    const next = interpretDj(prompt);
    setPlan(next);
    onEnergy(next.energy);
    setBusy(true);
    try {
      const results = await searchTracks(next.query);
      if (!results[0]) throw new Error("No matching Spotify tracks found");
      await playTrack(results[0]);
    } catch (error) {
      notify(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="max-w-3xl">
      <h2 className="text-2xl font-semibold flex items-center gap-2">
        <Music2 size={22} /> Spotify Player + DJ
      </h2>
      <p className="text-sm mt-2" style={{ color: "var(--muted)" }}>
        Official Spotify Web Playback SDK. Premium is required for in-app audio; Spotify handles the stream.
      </p>
      <div className="flex flex-wrap gap-2 mt-3">
        <button className="px-4 py-2 rounded-xl" style={{ background: "var(--accent)", color: "#081018" }} onClick={() => void connectSpotify()}>
          {token ? "Reconnect Spotify" : "Connect Spotify"}
        </button>
        {token && <button className="px-4 py-2 rounded-xl" style={{ border: "1px solid var(--stroke)" }} onClick={async () => {
          playerRef.current?.disconnect();
          playerRef.current = null;
          await api.spotifyDisconnect();
          setToken("");
          setSpotifyProfile(null);
          setPlayback(null);
          setTracks([]);
          notify("Spotify disconnected");
        }}>Disconnect</button>}
        <span className="text-sm self-center" style={{ color: deviceId ? "var(--ok)" : "var(--muted)" }}>
          {token ? (deviceId ? "Connected · player ready" : "Connected · starting player…") : "Not connected"}
        </span>
      </div>
      {spotifyProfile && (
        <div className="mt-4 flex items-center gap-3 p-3 rounded-xl" style={{ border: "1px solid var(--stroke)" }}>
          {spotifyProfile.imageUrl ? (
            <img className="h-11 w-11 rounded-full object-cover" src={spotifyProfile.imageUrl} alt="" />
          ) : (
            <div className="h-11 w-11 rounded-full grid place-items-center" style={{ background: "color-mix(in srgb, var(--accent) 18%, transparent)" }}><User size={18} /></div>
          )}
          <div className="min-w-0 flex-1">
            <div className="text-xs kicker">Spotify account</div>
            <div className="font-medium truncate">{spotifyProfile.displayName}</div>
            <div className="text-xs truncate" style={{ color: "var(--muted)" }}>{spotifyProfile.email || spotifyProfile.id}</div>
          </div>
          {spotifyProfile.product && <span className="text-xs capitalize" style={{ color: "var(--muted)" }}>{spotifyProfile.product}</span>}
        </div>
      )}
      {spotifyProfile && (
        <section className="mt-5" aria-label="Spotify playlists">
          <div className="flex items-center justify-between gap-3 mb-3">
            <div>
              <div className="kicker">Your playlists</div>
              {selectedPlaylist && <div className="text-sm mt-1">{selectedPlaylist.name} · {selectedPlaylist.tracks.total} tracks</div>}
            </div>
            {selectedPlaylist && <button className="px-3 py-2 rounded-lg text-sm" style={{ background: "var(--accent)", color: "var(--bg0)" }} disabled={!deviceId} onClick={() => void playPlaylist(selectedPlaylist)}>Play playlist</button>}
          </div>
          <div className="flex gap-2 overflow-x-auto pb-2">
            {playlists.map((playlist) => (
              <button key={playlist.id} className="playlist-tile" aria-pressed={selectedPlaylist?.id === playlist.id} onClick={() => void openPlaylist(playlist)}>
                {playlist.images[0] ? <img src={playlist.images[0].url} alt="" /> : <div className="playlist-placeholder"><Music2 size={20} /></div>}
                <span title={playlist.name}>{playlist.name}</span>
              </button>
            ))}
            {playlists.length === 0 && <p className="text-sm" style={{ color: "var(--muted)" }}>No playlists found.</p>}
          </div>
        </section>
      )}
      {playback?.track_window.current_track && (
        <div className="mt-5 flex items-center gap-4 p-4 rounded-2xl" style={{ border: "1px solid var(--stroke)" }}>
          {playback.track_window.current_track.album.images[0] && <img className="w-16 h-16 rounded-lg" src={playback.track_window.current_track.album.images[0].url} alt="Album cover" />}
          <div className="min-w-0 flex-1">
            <div className="font-medium truncate">{playback.track_window.current_track.name}</div>
            <div className="text-sm truncate" style={{ color: "var(--muted)" }}>{playback.track_window.current_track.artists.map((artist) => artist.name).join(", ")}</div>
          </div>
          <button aria-label="Previous track" title="Previous track" className="px-3 py-2 rounded-lg" style={{ border: "1px solid var(--stroke)" }} onClick={() => void playerRef.current?.previousTrack()}><SkipBack size={16} /></button>
          <button aria-label={playback.paused ? "Play" : "Pause"} title={playback.paused ? "Play" : "Pause"} className="px-3 py-2 rounded-lg" style={{ border: "1px solid var(--stroke)" }} onClick={() => void playerRef.current?.togglePlay()}><Play size={16} /></button>
          <button aria-label="Next track" title="Next track" className="px-3 py-2 rounded-lg" style={{ border: "1px solid var(--stroke)" }} onClick={() => void playerRef.current?.nextTrack()}><SkipForward size={16} /></button>
        </div>
      )}
      <div className="mt-5 p-4 rounded-2xl" style={{ border: "1px solid var(--stroke)" }}>
        <div className="kicker">DJ plan</div>
        <div className="text-lg mt-1">{plan.mood}</div>
        <div className="text-sm mt-1" style={{ color: "var(--muted)" }}>Search: {plan.query}</div>
        <textarea className="w-full mt-3 min-h-20" value={prompt} onChange={(event) => setPrompt(event.target.value)} />
        <div className="flex gap-2 mt-3">
          <button className="px-4 py-2 rounded-xl" style={{ background: "var(--accent)", color: "#081018" }} disabled={!token || busy} onClick={() => void playDjPick()}>{busy ? "Finding track…" : "Play DJ pick"}</button>
          <button className="px-4 py-2 rounded-xl" style={{ border: "1px solid var(--stroke)" }} disabled={!token || busy} onClick={() => void searchTracks(prompt).catch((error) => notify(String(error)))}>Search</button>
        </div>
      </div>
      <div className="mt-4 grid gap-2">
        {tracks.map((track) => (
          <div key={track.id} className="flex items-center gap-3 p-3 rounded-xl" style={{ border: "1px solid var(--stroke)" }}>
            {track.album.images[0] && <img className="w-11 h-11 rounded-md" src={track.album.images[0].url} alt="" />}
            <div className="min-w-0 flex-1">
              <div className="truncate">{track.name}</div>
              <div className="text-xs truncate" style={{ color: "var(--muted)" }}>{track.artists.map((artist) => artist.name).join(", ")}</div>
            </div>
            <button className="px-3 py-2 rounded-lg" style={{ border: "1px solid var(--stroke)" }} disabled={!deviceId} onClick={() => void playTrack(track).catch((error) => notify(String(error)))}>Play</button>
          </div>
        ))}
      </div>
    </div>
  );
}

function Studio({ notify }: { instanceId: string; notify: (s: string) => void }) {
  const [prompt, setPrompt] = useState("Steampunk airship dock");
  const [schematic, setSchematic] = useState<Schematic | null>(null);
  const [anim, setAnim] = useState<"idle" | "walk" | "run" | "pvp">("idle");
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [gap, setGap] = useState(2);
  const [size, setSize] = useState(4);
  const [color, setColor] = useState("#6ee7ff");

  const draw = () => {
    const c = canvasRef.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, c.width, c.height);
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    const cx = c.width / 2;
    const cy = c.height / 2;
    ctx.beginPath();
    ctx.moveTo(cx - size - gap, cy);
    ctx.lineTo(cx - gap, cy);
    ctx.moveTo(cx + gap, cy);
    ctx.lineTo(cx + size + gap, cy);
    ctx.moveTo(cx, cy - size - gap);
    ctx.lineTo(cx, cy - gap);
    ctx.moveTo(cx, cy + gap);
    ctx.lineTo(cx, cy + size + gap);
    ctx.stroke();
  };
  useEffect(draw, [gap, size, color]);

  return (
    <div className="grid grid-cols-2 gap-6">
      <div>
        <h3 className="text-xl font-semibold">Skin / cape preview</h3>
        <p className="text-sm" style={{ color: "var(--muted)" }}>
          CSS rig — not a generative model. Animated capes for other players need everyone on the companion mod.
        </p>
        <div className={`skin-rig ${anim} mt-6`}>
          <div className="skin-box" style={{ width: 40, height: 40, left: 40, top: 0, borderRadius: 8 }} />
          <div className="skin-box" style={{ width: 48, height: 70, left: 36, top: 44, borderRadius: 8 }} />
          <div className="skin-box" style={{ width: 18, height: 68, left: 14, top: 48 }} />
          <div className="skin-box" style={{ width: 18, height: 68, left: 88, top: 48 }} />
          <div className="skin-box" style={{ width: 20, height: 72, left: 36, top: 118 }} />
          <div className="skin-box" style={{ width: 20, height: 72, left: 64, top: 118 }} />
        </div>
        <div className="flex gap-2 mt-4">
          {(["idle", "walk", "run", "pvp"] as const).map((a) => (
            <button key={a} className="px-3 py-1 rounded-lg text-sm" style={{ border: "1px solid var(--stroke)" }} onClick={() => setAnim(a)}>
              {a}
            </button>
          ))}
        </div>
      </div>
      <div>
        <h3 className="text-xl font-semibold flex items-center gap-2">
          <Crosshair size={18} /> Crosshair studio
        </h3>
        <canvas ref={canvasRef} width={160} height={160} className="mt-3 rounded-2xl" style={{ background: "#000", border: "1px solid var(--stroke)" }} />
        <div className="flex gap-3 mt-3 text-sm">
          <label>Gap <input type="range" min={0} max={12} value={gap} onChange={(e) => setGap(+e.target.value)} /></label>
          <label>Size <input type="range" min={2} max={16} value={size} onChange={(e) => setSize(+e.target.value)} /></label>
          <input type="color" value={color} onChange={(e) => setColor(e.target.value)} />
        </div>
        <button
          className="mt-2 px-3 py-2 rounded-xl text-sm"
          style={{ border: "1px solid var(--stroke)" }}
          onClick={async () => {
            const url = canvasRef.current?.toDataURL("image/png");
            if (url) notify(await api.saveCrosshair(url));
          }}
        >
          Save PNG
        </button>
        <h3 className="text-xl font-semibold mt-8">Blueprint generator</h3>
        <input className="w-full mt-2" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
        <button
          className="mt-2 px-3 py-2 rounded-xl text-sm"
          style={{ background: "var(--accent)", color: "#081018" }}
          onClick={async () => setSchematic(await api.schematic(prompt))}
        >
          Generate layers
        </button>
        {schematic && (
          <pre className="mt-3 text-xs overflow-auto max-h-48 p-3 rounded-xl" style={{ background: "rgba(0,0,0,0.35)" }}>
            {JSON.stringify(schematic, null, 2)}
          </pre>
        )}
      </div>
    </div>
  );
}

function Profiler({ sys }: { instanceId: string; sys: SystemInfo | null }) {
  const [companionFps, setCompanionFps] = useState<number | null>(null);
  useEffect(() => {
    const unlisten = listen<number>("overlay-fps", (event) => setCompanionFps(event.payload));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  const estimates = [
    { name: "Sodium", fps: "+40–120%", cost: "Usually a gain" },
    { name: "Iris + Complementary", fps: "−20–60%", cost: "Shaders dominate GPU" },
    { name: "HD resource pack 256px", fps: "−5–15%", cost: "VRAM / atlas" },
    { name: "Entity-heavy mods", fps: "−10–40%", cost: "CPU tick + rendering" },
  ];
  return (
    <div>
      <h2 className="text-2xl font-semibold">FPS cost profiler</h2>
      <p className="text-sm mt-1" style={{ color: "var(--muted)" }}>
        Live per-mod frame timing requires the companion Fabric mod sampling Minecraft’s renderer.
        Hardware below is live from the launcher; costs are typical ranges until the overlay is connected.
      </p>
      {sys && (
        <div className="grid grid-cols-2 lg:grid-cols-5 gap-3 mt-4">
          <Stat label="CPU" value={sys.cpu_brand} />
          <Stat label="Cores" value={String(sys.cpu_cores)} />
          <Stat label="RAM" value={`${Math.round(sys.used_ram_mb)} / ${sys.total_ram_mb} MB`} />
          <Stat label="Arch" value={sys.arch} />
          <Stat label="Companion FPS" value={companionFps === null ? "No signal" : `${companionFps} FPS`} />
        </div>
      )}
      <div className="mt-5 grid gap-2">
        {estimates.map((e) => (
          <div key={e.name} className="p-3 rounded-2xl flex justify-between" style={{ border: "1px solid var(--stroke)" }}>
            <div>
              <div className="font-medium">{e.name}</div>
              <div className="text-xs" style={{ color: "var(--muted)" }}>{e.cost}</div>
            </div>
            <div>{e.fps}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

function Vault({ instanceId }: { instanceId: string }) {
  const [worlds, setWorlds] = useState<WorldSave[]>([]);
  const [play, setPlay] = useState<PlaytimeSummary | null>(null);
  useEffect(() => {
    if (instanceId) api.worlds(instanceId).then(setWorlds).catch(() => setWorlds([]));
    api.playtime().then(setPlay).catch(() => null);
  }, [instanceId]);
  return (
    <div>
      <h2 className="text-2xl font-semibold">World vault</h2>
      {play && (
        <div className="grid grid-cols-3 gap-3 mt-4">
          <Stat label="Lifetime" value={`${Math.round(play.lifetimeSeconds / 3600)} h`} />
          <Stat label="This week" value={`${Math.round(play.weekSeconds / 60)} min`} />
          <Stat label="Sessions" value={String(play.sessions)} />
        </div>
      )}
      <div className="mt-5">
        {worlds.length === 0 && <p style={{ color: "var(--muted)" }}>No saves in this instance yet.</p>}
        {worlds.map((w) => (
          <div key={w.path} className="p-3 rounded-2xl mt-2" style={{ border: "1px solid var(--stroke)" }}>
            <div className="font-medium">{w.name}</div>
            <div className="text-xs" style={{ color: "var(--muted)" }}>{w.path}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

function SettingsView({
  theme,
  setTheme,
  palette,
  setPalette,
  onWallpaper,
  onClearWallpaper,
  hasWallpaper,
  motionBlur,
  setMotionBlur,
  pinConfigured,
  setPinConfigured,
  setLocked,
  setPinInput,
  account,
  setAccount,
  notify,
  sys,
}: {
  theme: string;
  setTheme: (t: string) => void;
  palette: ThemePalette;
  setPalette: (palette: ThemePalette) => void;
  onWallpaper: (file: File) => Promise<void>;
  onClearWallpaper: () => Promise<void>;
  hasWallpaper: boolean;
  motionBlur: boolean;
  setMotionBlur: (v: boolean) => void;
  pinConfigured: boolean;
  setPinConfigured: (enabled: boolean) => void;
  setLocked: (locked: boolean) => void;
  setPinInput: (pin: string) => void;
  account: Account | null;
  setAccount: (a: Account | null) => void;
  notify: (s: string) => void;
  sys: SystemInfo | null;
}) {
  const [login, setLogin] = useState<string>("");
  const [pin, setPin] = useState("");
  const [currentPin, setCurrentPin] = useState("");
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [themeName, setThemeName] = useState(localStorage.getItem("nebula-theme-name") || "My Nebula");
  useEffect(() => {
    api.accounts().then(setAccounts).catch(() => setAccounts([]));
  }, [account?.uuid]);
  const exportTheme = () => {
    const blob = new Blob([JSON.stringify({ schemaVersion: 1, name: themeName, theme, palette, motionBlur }, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${themeName.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "nebula-theme"}.json`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  const importTheme = async (file?: File) => {
    if (!file) return;
    try {
      const imported = JSON.parse(await file.text()) as { name?: string; palette?: Partial<ThemePalette>; motionBlur?: boolean };
      const next = { ...palette, ...imported.palette };
      if (Object.values(next).some((value) => typeof value !== "string" || !/^#[\da-f]{6}$/i.test(value))) {
        throw new Error("Theme colors must use six-digit hex values");
      }
      setPalette(next);
      setTheme("custom");
      if (typeof imported.motionBlur === "boolean") setMotionBlur(imported.motionBlur);
      const name = imported.name?.trim() || file.name.replace(/\.json$/i, "");
      setThemeName(name);
      localStorage.setItem("nebula-theme-name", name);
      notify(`Imported ${name}`);
    } catch (error) {
      notify(`Theme import failed: ${String(error)}`);
    }
  };

  const updatePaletteColor = (key: keyof ThemePalette, value: string) => {
    const next = { ...palette, [key]: value };
    setPalette(next);
    setTheme("custom");
  };

  return (
    <div className="max-w-xl space-y-6">
      <h2 className="text-2xl font-semibold">Settings</h2>
      <div>
        <div className="kicker">Microsoft account</div>
        <p className="text-sm mt-1" style={{ color: "var(--muted)" }}>
          Device-code OAuth. Set <code>NEBULA_MS_CLIENT_ID</code> to your Azure public client that has Xbox Live / Minecraft APIs.
        </p>
        <p className="mt-2">{account ? `Active: ${account.name}` : "No account"}</p>
        <div className="mt-3 grid gap-2">
          {accounts.map((savedAccount) => (
            <div key={savedAccount.uuid} className="flex items-center gap-2 p-3 rounded-xl" style={{ border: "1px solid var(--stroke)" }}>
              <div className="min-w-0 flex-1">
                <div className="truncate">{savedAccount.name}</div>
                <div className="text-xs" style={{ color: "var(--muted)" }}>{savedAccount.uuid}</div>
              </div>
              <button
                className="px-3 py-2 rounded-lg text-sm"
                style={{ border: "1px solid var(--stroke)" }}
                disabled={savedAccount.uuid === account?.uuid}
                onClick={async () => {
                  try {
                    await api.setActive(savedAccount.uuid);
                    setAccount(savedAccount);
                    notify(`Active account: ${savedAccount.name}`);
                  } catch (error) {
                    notify(String(error));
                  }
                }}
              >
                {savedAccount.uuid === account?.uuid ? "Active" : "Use"}
              </button>
              <button
                className="px-3 py-2 rounded-lg text-sm"
                style={{ border: "1px solid var(--stroke)" }}
                onClick={async () => {
                  if (!window.confirm(`Remove ${savedAccount.name} from Nebula?`)) return;
                  try {
                    await api.removeAccount(savedAccount.uuid);
                    const [nextAccounts, activeAccount] = await Promise.all([api.accounts(), api.activeAccount()]);
                    setAccounts(nextAccounts);
                    setAccount(activeAccount);
                    notify(`Removed ${savedAccount.name}`);
                  } catch (error) {
                    notify(String(error));
                  }
                }}
              >
                Remove
              </button>
            </div>
          ))}
        </div>
        <button
          className="mt-2 px-4 py-2 rounded-xl"
          style={{ background: "var(--accent)", color: "#081018" }}
          onClick={async () => {
            try {
              const start = await api.startMsLogin();
              setLogin(`${start.message}\n${start.verificationUri} · ${start.userCode}`);
              notify("Complete login in the browser, then wait for poll…");
              const poll = async () => {
                try {
                  const acc = await api.pollMsLogin(start.deviceCode, start.clientId);
                  setAccount(acc);
                  notify(`Welcome ${acc.name}`);
                } catch (e) {
                  const msg = String(e);
                  if (msg.includes("authorization_pending") || msg.includes("slow_down")) {
                    setTimeout(poll, (start.interval || 5) * 1000);
                  } else notify(msg);
                }
              };
              setTimeout(poll, (start.interval || 5) * 1000);
            } catch (e) {
              notify(String(e));
            }
          }}
        >
          Sign in with Microsoft
        </button>
        {login && <pre className="text-xs mt-3 whitespace-pre-wrap">{login}</pre>}
      </div>
      <div>
        <div className="kicker">Theme</div>
        <div className="flex flex-wrap gap-2 mt-2">
          {["aurora", "ember", "moss", "mono"].map((t) => (
            <button key={t} className="px-3 py-2 rounded-xl capitalize" style={{ border: "1px solid var(--stroke)" }} onClick={() => setTheme(t)}>
              {t}
            </button>
          ))}
          <button className="px-3 py-2 rounded-xl capitalize" style={{ border: "1px solid var(--stroke)" }} onClick={() => setTheme("custom")}>
            Custom
          </button>
          <button className="px-3 py-2 rounded-xl" style={{ border: "1px solid var(--stroke)" }} onClick={exportTheme}>
            Export
          </button>
          <label className="px-3 py-2 rounded-xl cursor-pointer" style={{ border: "1px solid var(--stroke)" }}>
            Import
            <input className="sr-only" type="file" accept="application/json,.json" onChange={(event) => {
              void importTheme(event.target.files?.[0]);
              event.target.value = "";
            }} />
          </label>
        </div>
        <label className="block text-sm mt-3" style={{ color: "var(--muted)" }}>
          Palette name
          <input className="block w-full mt-1" value={themeName} onChange={(event) => {
            setThemeName(event.target.value);
            localStorage.setItem("nebula-theme-name", event.target.value);
          }} />
        </label>
        <div className="grid grid-cols-2 sm:grid-cols-3 gap-3 mt-4">
          {([
            ["accent", "Accent"],
            ["accent2", "Secondary accent"],
            ["bg0", "Background"],
            ["bg1", "Panel background"],
            ["text", "Text"],
            ["muted", "Muted text"],
          ] as Array<[keyof ThemePalette, string]>).map(([key, label]) => (
            <label key={key} className="flex items-center gap-3 text-sm p-2 rounded-lg" style={{ border: "1px solid var(--stroke)" }}>
              <input className="h-9 w-10 p-0.5" type="color" value={palette[key]} onChange={(event) => updatePaletteColor(key, event.target.value)} />
              <span>{label}</span>
            </label>
          ))}
        </div>
      </div>
      <div>
        <div className="kicker">Wallpaper</div>
        <div className="flex flex-wrap items-center gap-3 mt-2">
          <label className="px-3 py-2 rounded-xl cursor-pointer text-sm" style={{ border: "1px solid var(--stroke)" }}>
            Choose image, GIF, or MP4
            <input className="sr-only" type="file" accept="image/*,video/mp4" onChange={async (event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (!file) return;
              try {
                await onWallpaper(file);
                notify("Wallpaper saved");
              } catch (error) {
                notify(String(error));
              }
            }} />
          </label>
          {hasWallpaper && <button className="px-3 py-2 rounded-xl text-sm" style={{ border: "1px solid var(--stroke)" }} onClick={() => void onClearWallpaper().then(() => notify("Wallpaper removed")).catch((error) => notify(String(error)))}>Remove wallpaper</button>}
        </div>
        <p className="text-xs mt-2" style={{ color: "var(--muted)" }}>Files up to 40 MiB are stored locally in this app.</p>
      </div>
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={motionBlur} onChange={(e) => setMotionBlur(e.target.checked)} />
        UI motion blur / glass
      </label>
      <div>
        <div className="kicker">PIN lock</div>
        <p className="text-sm mt-1" style={{ color: "var(--muted)" }}>
          {pinConfigured ? "Nebula asks for this PIN on startup. Enter the current PIN to change or remove the lock." : "Set a 4-digit PIN to lock Nebula on startup."}
        </p>
        {pinConfigured && (
          <input
            className="mt-2 w-full"
            inputMode="numeric"
            autoComplete="current-password"
            maxLength={4}
            value={currentPin}
            onChange={(event) => setCurrentPin(event.target.value.replace(/\D/g, "").slice(0, 4))}
            placeholder="Current 4-digit PIN"
            aria-label="Current PIN"
          />
        )}
        <input
          className="mt-2 w-full"
          inputMode="numeric"
          autoComplete="new-password"
          maxLength={4}
          value={pin}
          onChange={(event) => setPin(event.target.value.replace(/\D/g, "").slice(0, 4))}
          placeholder="New 4-digit PIN"
          aria-label="New PIN"
        />
        <div className="flex flex-wrap gap-2 mt-2">
          <button
            className="px-3 py-2 rounded-xl"
            style={{ background: "var(--accent)", color: "#081018" }}
            disabled={pin.length !== 4 || (pinConfigured && currentPin.length !== 4)}
            onClick={async () => {
              try {
                if (pinConfigured) await api.changePin(currentPin, pin);
                else await api.setPin(pin);
                setPinConfigured(true);
                setPin("");
                setCurrentPin("");
                notify("PIN saved");
              } catch (error) {
                notify(`Could not save PIN: ${String(error)}`);
              }
            }}
          >
            {pinConfigured ? "Change PIN" : "Save PIN"}
          </button>
          {pinConfigured && (
            <button
              className="px-3 py-2 rounded-xl"
              style={{ border: "1px solid var(--stroke)" }}
              disabled={currentPin.length !== 4}
              onClick={async () => {
                try {
                  await api.clearPin(currentPin);
                  setPinConfigured(false);
                  setCurrentPin("");
                  setPin("");
                  setPinInput("");
                  setLocked(false);
                  notify("PIN lock removed");
                } catch (error) {
                  notify(`Could not remove PIN lock: ${String(error)}`);
                }
              }}
            >
              Remove PIN
            </button>
          )}
        </div>
      </div>
      <div>
        <div className="kicker">Java</div>
        <button
          className="mt-2 px-3 py-2 rounded-xl"
          style={{ border: "1px solid var(--stroke)" }}
          onClick={async () => {
            try {
              notify("Downloading Temurin 21…");
              await api.downloadTemurin(21);
              notify("Java 21 installed into launcher runtimes");
            } catch (e) {
              notify(String(e));
            }
          }}
        >
          Download Temurin 21 ({sys?.arch})
        </button>
      </div>
      <div>
        <div className="kicker">Companion QR</div>
        <p className="text-sm" style={{ color: "var(--muted)" }}>
          Pairing token is local-only. Mobile app is a separate client that talks to <code>127.0.0.1:17384</code> on your LAN.
        </p>
        <div className="mt-2 p-4 rounded-xl font-mono text-xs" style={{ border: "1px solid var(--stroke)" }}>
          nebula://pair?host=127.0.0.1&port=17384&token=local
        </div>
      </div>
    </div>
  );
}

function CommandPalette({
  instances,
  onClose,
  onTab,
  onInstance,
  onTheme,
  onMotionBlur,
  theme,
  motionBlur,
}: {
  instances: Instance[];
  onClose: () => void;
  onTab: (t: Tab) => void;
  onInstance: (id: string) => void;
  onTheme: (theme: string) => void;
  onMotionBlur: (enabled: boolean) => void;
  theme: string;
  motionBlur: boolean;
}) {
  const [q, setQ] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const items = useMemo(() => {
    const all: { key: string; label: string; keywords?: string; run: () => void }[] = [
      ...TABS.map((t) => ({ key: `tab-${t.id}`, label: `Go to ${t.label}`, run: () => onTab(t.id) })),
      ...instances.map((i) => ({
        key: `instance-${i.id}`,
        label: `Select instance: ${i.name}`,
        keywords: `${i.minecraftVersion} ${i.loader} ${i.profile} version`,
        run: () => onInstance(i.id),
      })),
      ...["aurora", "ember", "moss", "mono", "custom"].map((nextTheme) => ({
        key: `theme-${nextTheme}`,
        label: `Theme: ${nextTheme}`,
        run: () => onTheme(nextTheme),
      })),
      {
        key: "motion-blur-on",
        label: "Enable UI motion blur",
        keywords: "settings glass",
        run: () => onMotionBlur(true),
      },
      {
        key: "motion-blur-off",
        label: "Disable UI motion blur",
        keywords: "settings glass reduce motion",
        run: () => onMotionBlur(false),
      },
    ];
    const terms = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return all.filter((item) => {
      const searchable = `${item.label} ${item.keywords ?? ""}`.toLowerCase();
      return terms.every((term) => searchable.includes(term));
    });
  }, [q, instances, onTab, onInstance, onTheme, onMotionBlur]);

  const selectedIndex = Math.min(activeIndex, Math.max(items.length - 1, 0));
  const handleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" && items.length > 0) {
      event.preventDefault();
      setActiveIndex((index) => (index + 1) % items.length);
    } else if (event.key === "ArrowUp" && items.length > 0) {
      event.preventDefault();
      setActiveIndex((index) => (index - 1 + items.length) % items.length);
    } else if (event.key === "Enter" && items[selectedIndex]) {
      event.preventDefault();
      items[selectedIndex].run();
    }
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/50 grid place-items-start justify-items-center overflow-y-auto px-4 pt-[12vh] pb-6" onClick={onClose}>
      <div className="glass flex max-h-[76vh] w-[min(560px,100%)] min-w-0 flex-col p-3" onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          role="combobox"
          aria-label="Search launcher commands"
          aria-expanded="true"
          aria-controls="command-palette-results"
          aria-autocomplete="list"
          className="w-full min-w-0 shrink-0"
          placeholder="Search pages, instances, versions, and settings…"
          value={q}
          onKeyDown={handleKeyDown}
          onChange={(event) => {
            setQ(event.target.value);
            setActiveIndex(0);
          }}
        />
        <div id="command-palette-results" role="listbox" className="mt-2 min-h-0 overflow-y-auto">
          {items.map((item, index) => (
            <button
              key={item.key}
              role="option"
              aria-selected={index === selectedIndex}
              className="w-full text-left px-3 py-2 rounded-xl hover:bg-white/5"
              style={index === selectedIndex ? { background: "color-mix(in oklab, var(--accent) 14%, transparent)" } : undefined}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={item.run}
            >
              {item.label}
            </button>
          ))}
          {items.length === 0 && <div className="px-3 py-4 text-sm" style={{ color: "var(--muted)" }}>No matching commands.</div>}
        </div>
        <div className="px-3 pt-2 text-xs" style={{ color: "var(--muted)" }}>
          {theme} theme · motion blur {motionBlur ? "on" : "off"} · ↑↓ navigate · Enter select
        </div>
      </div>
    </div>
  );
}
