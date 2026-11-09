import { invoke } from "@tauri-apps/api/core";
import type {
  IsolationFacet,
  KernelInfo,
  PrepareResult,
  Profile,
  ProxyCheckResult,
  ProxyConfig,
  RunningInfo,
  Settings,
} from "./types";

export const api = {
  listProfiles: () => invoke<Profile[]>("list_profiles"),
  listGroups: () => invoke<string[]>("list_groups"),
  saveProfile: (profile: Profile) => invoke<Profile>("save_profile", { profile }),
  deleteProfile: (id: string) => invoke<void>("delete_profile", { id }),
  duplicateProfile: (id: string) => invoke<Profile>("duplicate_profile", { id }),
  prepareProfile: (id: string) => invoke<PrepareResult>("prepare_profile", { id }),
  launchProfile: (id: string) => invoke<void>("launch_profile", { id }),
  closeProfile: (id: string) => invoke<void>("close_profile", { id }),
  runningProfiles: () => invoke<RunningInfo[]>("running_profiles"),
  isolationFacets: () => invoke<IsolationFacet[]>("isolation_facets"),
  openAutomation: (id: string) => invoke<void>("open_automation", { id }),
  closeAutomation: (id: string) => invoke<void>("close_automation", { id }),
  checkProxy: (proxy: ProxyConfig) => invoke<ProxyCheckResult>("check_proxy", { proxy }),
  kernelInfo: () => invoke<KernelInfo>("kernel_info"),
  kernelFetch: (specifier?: string) =>
    invoke<KernelInfo>("kernel_fetch", { specifier: specifier ?? null }),
  kernelSet: (specifier: string) => invoke<KernelInfo>("kernel_set", { specifier }),
  kernelRemove: (specifier: string) => invoke<KernelInfo>("kernel_remove", { specifier }),
  exportProfiles: (ids: string[] | null, includeSecrets: boolean) =>
    invoke<string>("export_profiles", { ids, includeSecrets }),
  importProfiles: (content: string, regenerate: boolean) =>
    invoke<number>("import_profiles", { content, regenerate }),
  exportCookies: (id: string, format: string) => invoke<string>("export_cookies", { id, format }),
  importCookies: (id: string, format: string, content: string) =>
    invoke<number>("import_cookies", { id, format, content }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  diagnose: () =>
    invoke<{
      sidecarMode: string;
      sidecarDir: string | null;
      python: string | null;
      kernelVersion: string | null;
      kernelPath: string | null;
      error: string | null;
    }>("diagnose"),
  dataDir: () => invoke<string>("data_dir"),
  writeTextFile: (path: string, content: string) => invoke<void>("write_text_file", { path, content }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
};
