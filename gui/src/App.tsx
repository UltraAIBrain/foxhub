import { useEffect, useMemo, useState } from "react";
import { ConfigProvider, Layout, Menu, theme } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import { ProfileOutlined, SettingOutlined, ThunderboltOutlined } from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { dictionaries, I18nContext } from "./i18n";
import type { ProfileStatus } from "./types";
import { ProfilesPage } from "./pages/ProfilesPage";
import { KernelPage } from "./pages/KernelPage";
import { SettingsPage } from "./pages/SettingsPage";

export type PageKey = "profiles" | "kernel" | "settings";

export interface RunningState {
  status: ProfileStatus["status"];
  pid?: number;
  endpoint?: string;
}

export function App() {
  const [page, setPage] = useState<PageKey>("profiles");
  const [language, setLanguage] = useState("zh-CN");
  const [running, setRunning] = useState<Record<string, RunningState>>({});

  useEffect(() => {
    api
      .getSettings()
      .then((s) => setLanguage(s.language || "zh-CN"))
      .catch(() => undefined);
    // Subscribe before the first fetch: the startup recovery scan reports
    // browsers a previous session left running, and an emit landing between
    // the fetch and the subscription would be lost.
    const unlisten = listen<ProfileStatus>("profile-status", (event) => {
      const s = event.payload;
      setRunning((prev) => {
        const next = { ...prev };
        if (s.status === "exited" || s.status === "failed") {
          delete next[s.id];
        } else {
          next[s.id] = { status: s.status, pid: s.pid, endpoint: s.endpoint };
        }
        return next;
      });
    });
    api
      .runningProfiles()
      .then((infos) =>
        setRunning((prev) =>
          Object.fromEntries([
            ...Object.entries(prev),
            ...infos.map((i) => [i.id, { status: "running" as const, pid: i.pid }]),
          ]),
        ),
      )
      .catch(() => undefined);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const dict = useMemo(() => dictionaries[language] ?? dictionaries["zh-CN"], [language]);
  const antdLocale = language === "en" ? enUS : zhCN;

  return (
    <I18nContext.Provider value={dict}>
      <ConfigProvider
        locale={antdLocale}
        theme={{ algorithm: theme.defaultAlgorithm, token: { colorPrimary: "#e8590c" } }}
      >
        <Layout className="camfox-layout">
          <Layout.Sider theme="dark" width={180} className="camfox-sider">
            <div className="logo">🦊 CamFox</div>
            <Menu
              theme="dark"
              mode="inline"
              selectedKeys={[page]}
              onClick={(e) => setPage(e.key as PageKey)}
              items={[
                { key: "profiles", icon: <ProfileOutlined />, label: dict.navProfiles },
                { key: "kernel", icon: <ThunderboltOutlined />, label: dict.navKernel },
                { key: "settings", icon: <SettingOutlined />, label: dict.navSettings },
              ]}
            />
          </Layout.Sider>
          <Layout.Content className="camfox-content">
            {page === "profiles" && <ProfilesPage running={running} />}
            {page === "kernel" && <KernelPage />}
            {page === "settings" && (
              <SettingsPage language={language} onLanguage={setLanguage} />
            )}
          </Layout.Content>
        </Layout>
      </ConfigProvider>
    </I18nContext.Provider>
  );
}
