// Types mirroring the Rust backend (camelCase on the wire).

export interface ScreenSpec {
  width: number;
  height: number;
}

export interface ProxyConfig {
  kind: "none" | "http" | "socks5";
  host: string;
  port: number;
  username: string;
  password: string;
}

export interface Summary {
  ua?: string | null;
  platform?: string | null;
  screen?: number[] | null;
  window?: number[] | null;
  timezone?: string | null;
  locale?: string | null;
  webrtcIpv4?: string | null;
  ip?: string | null;
  hardwareConcurrency?: number | null;
  devicePixelRatio?: number | null;
}

export interface Profile {
  id: string;
  name: string;
  group: string;
  notes: string;
  createdAt: number;
  lastLaunchedAt: number | null;
  launchCount: number;
  fingerprintMode: "fixed" | "random";
  os: string | null;
  screen: ScreenSpec | null;
  window: ScreenSpec | null;
  locale: string | null;
  timezone: string | null;
  geoip: boolean;
  proxy: ProxyConfig;
  automation: boolean;
  config?: object | null;
  prefs?: object | null;
  launchExecutable?: string | null;
  launchArgs: string[];
  extraEnv: Record<string, string>;
  summary?: Summary | null;
}

export interface RunningInfo {
  id: string;
  kind: "browser" | "automation";
  pid: number;
  wsEndpoint?: string | null;
}

export interface ProfileStatus {
  id: string;
  status: "launching" | "running" | "exited" | "failed" | "automation-connecting" | "automation-ready";
  pid?: number;
  endpoint?: string;
  error?: string;
}

export interface ProxyCheckResult {
  ip?: string | null;
  timezone?: string | null;
  locale?: string | null;
  latitude?: number | null;
  longitude?: number | null;
  geoipError?: string | null;
  error?: string | null;
}

export interface KernelInfo {
  installDir: string;
  activePath: string | null;
  version: string | null;
  installed: {
    label: string;
    channel: string;
    relativePath: string;
    path: string;
    isActive: boolean;
    isPrerelease: boolean;
    assetSize?: number | null;
    createdAt?: string | null;
  }[];
}

export interface Settings {
  pythonPath: string;
  sidecarPath: string;
  kernelExecutable: string;
  language: string;
  closeBrowsersOnExit: boolean;
}

export function emptyProxy(): ProxyConfig {
  return { kind: "none", host: "", port: 0, username: "", password: "" };
}

export function emptyProfile(): Profile {
  return {
    id: "",
    name: "",
    group: "",
    notes: "",
    createdAt: 0,
    lastLaunchedAt: null,
    launchCount: 0,
    fingerprintMode: "fixed",
    os: "windows",
    screen: null,
    window: null,
    locale: null,
    timezone: null,
    geoip: false,
    proxy: emptyProxy(),
    automation: false,
    config: null,
    prefs: null,
    launchExecutable: null,
    launchArgs: [],
    extraEnv: {},
    summary: null,
  };
}
