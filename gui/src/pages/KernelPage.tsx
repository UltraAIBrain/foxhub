import { useEffect, useRef, useState } from "react";
import { Button, Descriptions, Input, message, Space, Table, Typography } from "antd";
import { CloudDownloadOutlined, ReloadOutlined } from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import { useT } from "../i18n";
import type { KernelInfo } from "../types";

export function KernelPage() {
  const t = useT();
  const [info, setInfo] = useState<KernelInfo | null>(null);
  const [specifier, setSpecifier] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);

  const reload = async () => {
    try {
      setInfo(await api.kernelInfo());
    } catch (e) {
      message.error(String(e));
    }
  };

  useEffect(() => {
    reload();
    const unlisten = listen<{ line: string }>("kernel-log", (event) => {
      setLog((prev) => [...prev.slice(-500), event.payload.line]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [log]);

  const doFetch = async () => {
    setBusy(true);
    setLog([]);
    try {
      await api.kernelFetch(specifier.trim() || undefined);
      message.success(t.kernelDownload);
      await reload();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doSetActive = async (channel: string) => {
    try {
      await api.kernelSet(channel);
      await reload();
      message.success(t.kernelSetActive);
    } catch (e) {
      message.error(String(e));
    }
  };

  const doRemove = async (channel: string) => {
    try {
      await api.kernelRemove(channel);
      await reload();
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <div>
      <Space direction="vertical" style={{ width: "100%" }} size="large">
        <Descriptions
          bordered
          size="small"
          column={2}
          title={t.kernelActive}
          items={[
            { key: "v", label: t.kernelVersion, children: info?.version ?? "—" },
            {
              key: "p",
              label: t.kernelExecutable,
              children: (
                <Typography.Text copyable style={{ fontSize: 12 }}>
                  {info?.activePath ?? "—"}
                </Typography.Text>
              ),
            },
            { key: "d", label: t.kernelInstallDir, span: 2, children: info?.installDir ?? "—" },
          ]}
        />
        <Space wrap>
          <Input
            style={{ width: 320 }}
            placeholder={t.kernelSpecifier}
            value={specifier}
            onChange={(e) => setSpecifier(e.target.value)}
          />
          <Button type="primary" icon={<CloudDownloadOutlined />} loading={busy} onClick={doFetch}>
            {t.kernelDownload}
          </Button>
          <Button icon={<ReloadOutlined />} onClick={reload}>
            {t.kernelRefresh}
          </Button>
        </Space>
        <Table
          rowKey="relativePath"
          size="small"
          pagination={false}
          dataSource={info?.installed ?? []}
          columns={[
            { title: t.kernelVersion, dataIndex: "label", key: "label" },
            {
              title: t.kernelInstallDir,
              dataIndex: "path",
              key: "path",
              ellipsis: true,
              render: (v: string) => (
                <Typography.Text style={{ fontSize: 12 }} copyable>
                  {v}
                </Typography.Text>
              ),
            },
            {
              title: t.kernelSize,
              dataIndex: "assetSize",
              key: "size",
              width: 100,
              render: (v?: number | null) => (v ? `${(v / 1024 / 1024).toFixed(0)} MB` : "—"),
            },
            {
              title: t.colActions,
              key: "act",
              width: 200,
              render: (_: unknown, row: KernelInfo["installed"][number]) => (
                <Space>
                  <Button
                    size="small"
                    type={row.isActive ? "default" : "primary"}
                    disabled={row.isActive}
                    onClick={() => doSetActive(row.channel)}
                  >
                    {t.kernelSetActive}
                  </Button>
                  <Button size="small" danger disabled={row.isActive} onClick={() => doRemove(row.channel)}>
                    {t.kernelRemove}
                  </Button>
                </Space>
              ),
            },
          ]}
        />
        {log.length > 0 && (
          <>
            <Typography.Text strong>{t.kernelLog}</Typography.Text>
            <div className="camfox-log" ref={logRef}>
              {log.join("\n")}
            </div>
          </>
        )}
      </Space>
    </div>
  );
}
