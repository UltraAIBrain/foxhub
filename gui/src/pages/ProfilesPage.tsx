import { useEffect, useMemo, useState, type Key } from "react";
import {
  Button,
  Input,
  message,
  Popconfirm,
  Segmented,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import {
  ApiOutlined,
  CopyOutlined,
  DeleteOutlined,
  EditOutlined,
  ExportOutlined,
  ImportOutlined,
  LoadingOutlined,
  PlayCircleFilled,
  PlusOutlined,
  SafetyOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { save as saveDialog, open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useT } from "../i18n";
import type { Profile } from "../types";
import type { RunningState } from "../App";
import { ProfileEditModal } from "../components/ProfileEditModal";
import { CookieModal } from "../components/CookieModal";
import { IsolationModal } from "../components/IsolationModal";

const PAGE = "profiles";

export function ProfilesPage(props: { running: Record<string, RunningState> }) {
  const t = useT();
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [groups, setGroups] = useState<string[]>([]);
  const [search, setSearch] = useState("");
  const [groupFilter, setGroupFilter] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Key[]>([]);
  const [editTarget, setEditTarget] = useState<Profile | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [cookieTarget, setCookieTarget] = useState<Profile | null>(null);
  const [cookieOpen, setCookieOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [batchBusy, setBatchBusy] = useState<null | "launch" | "close">(null);
  const [isoOpen, setIsoOpen] = useState(false);
  const [page, setPage] = useState(1);

  const reload = async () => {
    try {
      const [list, gs] = await Promise.all([api.listProfiles(), api.listGroups()]);
      setProfiles(list);
      setGroups(gs);
    } catch (e) {
      message.error(String(e));
    }
  };

  useEffect(() => {
    reload();
  }, []);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return profiles.filter((p) => {
      if (groupFilter && p.group !== groupFilter) return false;
      if (!q) return true;
      return [p.name, p.group, p.notes].some((s) => s.toLowerCase().includes(q));
    });
  }, [profiles, search, groupFilter]);

  const running = props.running;

  // Deleting the last row of the last page would otherwise leave an empty
  // page selected, and the table would look as if it had lost every profile.
  useEffect(() => {
    const lastPage = Math.max(1, Math.ceil(filtered.length / 20));
    if (page > lastPage) setPage(lastPage);
  }, [filtered.length, page]);

  const doLaunch = async (p: Profile) => {
    try {
      await api.launchProfile(p.id);
      // The backend stamps lastLaunchedAt; without this the column stays stale
      // until something else reloads the list.
      await reload();
    } catch (e) {
      message.error(String(e));
    }
  };

  const doClose = async (p: Profile) => {
    try {
      await api.closeProfile(p.id);
    } catch (e) {
      message.error(String(e));
    }
  };

  const doAutomation = async (p: Profile) => {
    const state = running[p.id];
    if (state?.status === "automation-connecting" || state?.status === "automation-ready") {
      try {
        await api.closeAutomation(p.id);
      } catch (e) {
        message.error(String(e));
      }
      return;
    }
    try {
      await api.openAutomation(p.id);
    } catch (e) {
      message.error(String(e));
    }
  };

  const doDuplicate = async (p: Profile) => {
    setBusy(true);
    const key = `dup-${p.id}`;
    message.loading({ content: t.generating, key, duration: 0 });
    try {
      const copy = await api.duplicateProfile(p.id);
      await api.prepareProfile(copy.id);
      await reload();
      message.success({ content: t.duplicated, key });
    } catch (e) {
      message.error({ content: String(e), key });
    } finally {
      setBusy(false);
    }
  };

  const doDelete = async (p: Profile) => {
    try {
      await api.deleteProfile(p.id);
      await reload();
    } catch (e) {
      message.error(String(e));
    }
  };

  const doExport = async () => {
    try {
      const path = await saveDialog({
        title: t.exportProfiles,
        defaultPath: "camfox-profiles.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const ids = selectedIds.length > 0 ? selectedIds.map(String) : null;
      const content = await api.exportProfiles(ids, false);
      await api.writeTextFile(path, content);
      message.success(t.exportProfiles);
    } catch (e) {
      message.error(String(e));
    }
  };

  const doImport = async () => {
    try {
      const path = await openDialog({
        title: t.importProfiles,
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const content = await api.readTextFile(path);
      setBusy(true);
      message.loading({ content: t.generating, key: PAGE, duration: 0 });
      const count = await api.importProfiles(content, true);
      await reload();
      message.success({ content: `${t.importProfiles}: ${count}`, key: PAGE });
    } catch (e) {
      message.error({ content: String(e), key: PAGE });
    } finally {
      setBusy(false);
    }
  };

  const runBatch = async (mode: "launch" | "close") => {
    if (selectedIds.length === 0) {
      message.warning(t.selectRows);
      return;
    }
    const targets = selectedIds
      .map((id) => profiles.find((x) => x.id === id))
      .filter((p): p is Profile => !!p)
      .filter((p) => (mode === "launch" ? !running[p.id] : !!running[p.id]));
    if (targets.length === 0) return;

    const key = `batch-${mode}`;
    setBatchBusy(mode);
    message.loading({ content: t.generating, key, duration: 0 });
    try {
      // Sequential launching spent one wait per profile before starting the
      // next, so a batch of ten looked frozen for minutes. Each launch has its
      // own guard in the backend, so they can start together.
      const results = await Promise.allSettled(
        targets.map((p) => (mode === "launch" ? api.launchProfile(p.id) : api.closeProfile(p.id))),
      );
      const failed = results.filter((r) => r.status === "rejected").length;
      await reload();
      if (failed === 0) {
        message.success({ content: t.batchDone.replace("{n}", String(targets.length)), key });
      } else {
        message.warning({
          content: t.batchPartial
            .replace("{n}", String(targets.length - failed))
            .replace("{m}", String(failed)),
          key,
        });
      }
    } finally {
      setBatchBusy(null);
    }
  };

  const statusOf = (p: Profile): RunningState | undefined => running[p.id];

  const statusTag = (p: Profile) => {
    const s = statusOf(p);
    if (!s) return <Tag>{t.statusClosed}</Tag>;
    switch (s.status) {
      case "launching":
        return (
          <Tag color="orange" icon={<LoadingOutlined />}>
            {t.statusLaunching}
          </Tag>
        );
      case "running":
        return <Tag color="green">{t.statusRunning}</Tag>;
      case "automation-connecting":
        return <Tag color="orange">{t.statusConnecting}</Tag>;
      case "automation-ready":
        return (
          <Tooltip title={s.endpoint}>
            <Tag
              color="blue"
              icon={<CopyOutlined />}
              onClick={() => {
                if (!s.endpoint) return;
                navigator.clipboard.writeText(s.endpoint).then(() => message.success(t.copied));
              }}
            >
              {t.statusReady}
            </Tag>
          </Tooltip>
        );
      case "failed":
        return <Tag color="red">{t.statusFailed}</Tag>;
      default:
        return <Tag>{t.statusClosed}</Tag>;
    }
  };

  const columns = [
    {
      title: t.colName,
      dataIndex: "name",
      key: "name",
      width: 170,
      render: (_: unknown, p: Profile) => (
        <Tooltip title={p.notes}>
          <Space>
            <Typography.Text strong style={{ maxWidth: 110 }} ellipsis={{ tooltip: p.name }}>
              {p.name}
            </Typography.Text>
            {p.fingerprintMode === "random" && <Tag>{t.randomTag}</Tag>}
          </Space>
        </Tooltip>
      ),
    },
    { title: t.colGroup, dataIndex: "group", key: "group", width: 100, ellipsis: true },
    {
      title: t.colFingerprint,
      key: "fp",
      render: (_: unknown, p: Profile) => {
        if (!p.summary)
          return (
            <Tooltip title={t.needIdentityHint}>
              <Tag color="warning">{t.needIdentity}</Tag>
            </Tooltip>
          );
        return (
          <Typography.Text style={{ fontSize: 12 }} ellipsis={{ tooltip: p.summary.ua }}>
            {p.summary.platform ?? "?"} · {p.summary.screen?.[0]}×{p.summary.screen?.[1]}
            {p.summary.timezone ? ` · ${p.summary.timezone}` : ""}
          </Typography.Text>
        );
      },
    },
    {
      title: t.colProxy,
      key: "proxy",
      width: 100,
      render: (_: unknown, p: Profile) => {
        const kind = p.proxy.kind;
        if (kind === "none")
          return (
            <Tooltip title={t.noProxyHint}>
              <Tag color="orange">{t.hostIpTag}</Tag>
            </Tooltip>
          );
        const label = kind.toUpperCase();
        return (
          <Tooltip title={`${p.proxy.host}:${p.proxy.port}`}>
            <Tag color="purple">{label}</Tag>
          </Tooltip>
        );
      },
    },
    {
      title: t.colLastLaunch,
      dataIndex: "lastLaunchedAt",
      key: "last",
      width: 140,
      render: (v: number | null) =>
        v ? new Date(v * 1000).toLocaleString() : <Typography.Text type="secondary">{t.neverLaunched}</Typography.Text>,
    },
    { title: t.colStatus, key: "status", width: 100, render: (_: unknown, p: Profile) => statusTag(p) },
    {
      title: t.colActions,
      key: "actions",
      width: 280,
      render: (_: unknown, p: Profile) => {
        const s = statusOf(p);
        const isRunning = !!s;
        const isLaunching = s?.status === "launching";
        const isAutomation = s?.status === "automation-connecting" || s?.status === "automation-ready";
        return (
          <Space size={0} wrap>
            {isRunning ? (
              <Button
                type="link"
                size="small"
                icon={<StopOutlined />}
                disabled={isLaunching}
                onClick={() => doClose(p)}
              >
                {t.stop}
              </Button>
            ) : (
              <Button
                type="link"
                size="small"
                icon={<PlayCircleFilled />}
                disabled={isLaunching}
                onClick={() => doLaunch(p)}
              >
                {t.launch}
              </Button>
            )}
            {p.automation && (
              <Button type="link" size="small" icon={<ApiOutlined />} onClick={() => doAutomation(p)}>
                {isAutomation ? t.stop : t.automation}
              </Button>
            )}
            <Button
              type="link"
              size="small"
              icon={<EditOutlined />}
              onClick={() => {
                setEditTarget(p);
                setEditOpen(true);
              }}
            >
              {t.edit}
            </Button>
            <Button type="link" size="small" disabled={busy} onClick={() => doDuplicate(p)}>
              {t.duplicate}
            </Button>
            <Button
              type="link"
              size="small"
              disabled={isRunning}
              onClick={() => {
                setCookieTarget(p);
                setCookieOpen(true);
              }}
            >
              {t.cookies}
            </Button>
            <Popconfirm title={t.confirmDelete} onConfirm={() => doDelete(p)}>
              <Button type="link" size="small" danger disabled={isRunning} icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        );
      },
    },
  ];

  return (
    <div>
      <div className="camfox-toolbar">
        <Button
          type="primary"
          icon={<PlusOutlined />}
          onClick={() => {
            setEditTarget(null);
            setEditOpen(true);
          }}
        >
          {t.newProfile}
        </Button>
        <Input.Search
          placeholder={t.searchPlaceholder}
          allowClear
          style={{ width: 240 }}
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setPage(1);
          }}
        />
        <Segmented
          value={groupFilter ?? ""}
          onChange={(v) => {
            setGroupFilter((v as string) || null);
            setPage(1);
          }}
          options={[{ label: t.colGroup + ": " + t.none, value: "" }, ...groups.map((g) => ({ label: g, value: g }))]}
        />
        <Space style={{ marginLeft: "auto" }}>
          <Button icon={<PlayCircleFilled />} loading={batchBusy === "launch"} disabled={batchBusy !== null} onClick={() => runBatch("launch")}>
            {t.batchLaunch}
          </Button>
          <Button icon={<StopOutlined />} loading={batchBusy === "close"} disabled={batchBusy !== null} onClick={() => runBatch("close")}>
            {t.batchClose}
          </Button>
          <Button icon={<ImportOutlined />} loading={busy} onClick={doImport}>
            {t.importProfiles}
          </Button>
          <Button icon={<ExportOutlined />} onClick={doExport}>
            {t.exportProfiles}
          </Button>
          <Button icon={<SafetyOutlined />} onClick={() => setIsoOpen(true)}>
            {t.isolation}
          </Button>
        </Space>
      </div>
      <Table
        rowKey="id"
        columns={columns}
        dataSource={filtered}
        rowSelection={{ selectedRowKeys: selectedIds, onChange: setSelectedIds }}
        pagination={{ current: page, pageSize: 20, showSizeChanger: false, onChange: setPage }}
        size="middle"
      />
      <ProfileEditModal
        open={editOpen}
        profile={editTarget}
        groups={groups}
        onClose={() => setEditOpen(false)}
        onChanged={async () => {
          await reload();
        }}
      />
      {cookieTarget && (
        <CookieModal
          profile={cookieTarget}
          open={cookieOpen}
          onClose={() => setCookieOpen(false)}
        />
      )}
      <IsolationModal open={isoOpen} onClose={() => setIsoOpen(false)} />
    </div>
  );
}
