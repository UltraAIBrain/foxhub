import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Alert, Modal, Table, Tag, Tooltip, Typography, message } from "antd";
import { api } from "../api";
import { useT } from "../i18n";
import type { IsolationFacet } from "../types";

/** Values a page can use to link two profiles on this machine. */
type FacetKey = "gpu" | "screen" | "timezone" | "proxy";

const FACETS: FacetKey[] = ["gpu", "screen", "timezone", "proxy"];

/** "ANGLE (Intel, Intel(R) HD Graphics Direct3D11 vs_5_0 ps_5_0), or similar" */
function shortGpu(gpu: string): string {
  const open = gpu.indexOf("(");
  const close = gpu.lastIndexOf(")");
  if (open < 0 || close <= open) return gpu;
  const parts = gpu.slice(open + 1, close).split(",");
  return parts[parts.length - 1].replace(/\s*(vs_|ps_).*$/, "").trim();
}

export function IsolationModal(props: { open: boolean; onClose: () => void }) {
  const t = useT();
  const [rows, setRows] = useState<IsolationFacet[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!props.open) return;
    setLoading(true);
    api
      .isolationFacets()
      .then(setRows)
      .catch((e) => message.error(String(e)))
      .finally(() => setLoading(false));
  }, [props.open]);

  const shared = useMemo(() => {
    const keyOf = (f: IsolationFacet, facet: FacetKey): string | null => {
      switch (facet) {
        case "gpu":
          // Canvas and WebGL pixels follow both, so the pair is the unit.
          return f.gpu ? `${f.os ?? ""}|${f.gpu}` : null;
        case "screen":
          return f.screen ? f.screen.join("x") : null;
        case "timezone":
          return f.timezone;
        case "proxy":
          return f.proxy === "none" ? "@host" : f.proxyHost;
      }
    };
    const maps = {} as Record<FacetKey, Map<string, string[]>>;
    for (const facet of FACETS) maps[facet] = new Map();
    for (const f of rows) {
      for (const facet of FACETS) {
        const key = keyOf(f, facet);
        if (!key) continue;
        maps[facet].set(key, [...(maps[facet].get(key) ?? []), f.name]);
      }
    }
    return {
      peersOf(f: IsolationFacet, facet: FacetKey): string[] {
        const key = keyOf(f, facet);
        if (!key) return [];
        return (maps[facet].get(key) ?? []).filter((n) => n !== f.name);
      },
      groups: FACETS.reduce(
        (n, facet) => n + [...maps[facet].values()].filter((v) => v.length > 1).length,
        0,
      ),
    };
  }, [rows]);

  const cell = (f: IsolationFacet, facet: FacetKey, text: ReactNode) => {
    const peers = shared.peersOf(f, facet);
    if (peers.length === 0) return text;
    return (
      <Tooltip title={`${t.isolationSharedWith}: ${peers.join(", ")}`}>
        <Tag color="warning">{text}</Tag>
      </Tooltip>
    );
  };

  const gpu = (f: IsolationFacet) => {
    const full = f.gpu ?? "—";
    return cell(
      f,
      "gpu",
      <Typography.Text style={{ fontSize: 12 }} ellipsis={{ tooltip: full }}>
        {shortGpu(full)}
      </Typography.Text>,
    );
  };

  const osLabel = (f: IsolationFacet) =>
    f.platform ??
    ({ windows: "Windows", macos: "macOS", linux: "Linux" } as Record<string, string>)[
      f.os ?? ""
    ] ??
    f.os ??
    "—";

  const columns = [
    { title: t.colName, dataIndex: "name", key: "name", width: 130, ellipsis: true },
    {
      title: t.fieldOs,
      key: "os",
      width: 100,
      render: (_: unknown, f: IsolationFacet) => osLabel(f),
    },
    // Before the geometry columns: sharing an OS and GPU is what makes two
    // profiles produce the same canvas and WebGL pixels.
    { title: t.gpu, key: "gpu", width: 190, render: (_: unknown, f: IsolationFacet) => gpu(f) },
    {
      title: t.fieldScreen,
      key: "screen",
      width: 115,
      render: (_: unknown, f: IsolationFacet) =>
        cell(f, "screen", f.screen ? f.screen.join("×") : "—"),
    },
    {
      title: t.fieldTimezone,
      key: "timezone",
      width: 145,
      render: (_: unknown, f: IsolationFacet) => cell(f, "timezone", f.timezone ?? "—"),
    },
    {
      title: t.fieldLocale,
      key: "locale",
      width: 85,
      render: (_: unknown, f: IsolationFacet) => f.locale ?? "—",
    },
    {
      title: t.cores,
      key: "cores",
      width: 65,
      render: (_: unknown, f: IsolationFacet) => f.cores ?? "—",
    },
    {
      title: t.colProxy,
      key: "proxy",
      width: 130,
      render: (_: unknown, f: IsolationFacet) =>
        cell(f, "proxy", f.proxy === "none" ? t.hostIpTag : f.proxyHost ?? f.proxy),
    },
  ];

  return (
    <Modal open={props.open} onCancel={props.onClose} footer={null} width={1080} title={t.isolation}>
      <Alert
        type={shared.groups > 0 ? "warning" : "success"}
        showIcon
        style={{ marginBottom: 12 }}
        message={
          shared.groups > 0
            ? t.isolationConflicts.replace("{n}", String(shared.groups))
            : t.isolationClean
        }
        description={t.isolationHint}
      />
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={rows}
        columns={columns}
        pagination={false}
        scroll={{ y: 420 }}
      />
      <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginTop: 12, marginBottom: 0 }}>
        {t.isolationAudioNote}
      </Typography.Paragraph>
    </Modal>
  );
}
