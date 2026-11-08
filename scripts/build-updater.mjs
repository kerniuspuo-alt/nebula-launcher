import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const localEnv = await readFile(new URL("../.env", import.meta.url), "utf8").catch((error) => {
  if (error.code === "ENOENT") return "";
  throw error;
});
const localValues = Object.fromEntries(
  localEnv.split(/\r?\n/).flatMap((line) => {
    const match = line.match(/^\s*(NEBULA_UPDATER_ENDPOINT|TAURI_SIGNING_PRIVATE_KEY)\s*=\s*(.*?)\s*$/);
    if (!match) return [];
    let value = match[2];
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    return [[match[1], value]];
  }),
);
const tauriConfig = JSON.parse(await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const endpoint = (process.env.NEBULA_UPDATER_ENDPOINT || localValues.NEBULA_UPDATER_ENDPOINT)?.trim();
const publicKey = process.env.NEBULA_UPDATER_PUBLIC_KEY?.trim()
  || tauriConfig.plugins?.updater?.pubkey?.trim();
const privateKey = (process.env.TAURI_SIGNING_PRIVATE_KEY || localValues.TAURI_SIGNING_PRIVATE_KEY)?.trim();

const missing = [];
if (!endpoint) missing.push("NEBULA_UPDATER_ENDPOINT (a hosted HTTPS latest.json manifest)");
if (!publicKey) missing.push("updater public key in src-tauri/tauri.conf.json");
if (!privateKey) missing.push("TAURI_SIGNING_PRIVATE_KEY in .env or the build environment");
if (missing.length) {
  console.error(`Updater release build is missing: ${missing.join("; ")}.`);
  process.exit(1);
}

let parsedEndpoint;
try {
  parsedEndpoint = new URL(endpoint);
} catch {
  console.error("NEBULA_UPDATER_ENDPOINT must be a valid HTTPS URL.");
  process.exit(1);
}
if (parsedEndpoint.protocol !== "https:") {
  console.error("Updater endpoints must use HTTPS.");
  process.exit(1);
}
if (parsedEndpoint.hostname === "example.com" || parsedEndpoint.hostname.endsWith(".example.com")) {
  console.error("Replace the example updater endpoint with a real hosted HTTPS latest.json URL.");
  process.exit(1);
}

const tempDir = await mkdtemp(join(tmpdir(), "nebula-updater-build-"));
const configPath = join(tempDir, "tauri.updater.conf.json");
const config = {
  bundle: { createUpdaterArtifacts: true },
  plugins: {
    updater: {
      endpoints: [endpoint],
      pubkey: publicKey,
      requireSignedVersion: true,
      dangerousInsecureTransportProtocol: false,
    },
  },
};

try {
  await writeFile(configPath, JSON.stringify(config), { mode: 0o600 });
  const result = spawnSync(
    "npm",
    ["run", "tauri", "--", "build", "--config", configPath],
    {
      stdio: "inherit",
      env: {
        ...process.env,
        TAURI_SIGNING_PRIVATE_KEY: privateKey,
        VITE_TAURI_UPDATER_ENABLED: "true",
      },
    },
  );
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
