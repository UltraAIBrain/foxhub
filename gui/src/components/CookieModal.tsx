import { useState } from "react";
import { Alert, Button, message, Modal, Radio, Space } from "antd";
import { DownloadOutlined, UploadOutlined } from "@ant-design/icons";
import { save, open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useT } from "../i18n";
import type { Profile } from "../types";

export function CookieModal(props: { profile: Profile; open: boolean; onClose: () => void }) {
  const t = useT();
  const [format, setFormat] = useState<"json" | "netscape">("json");
  const [busy, setBusy] = useState(false);

  const doExport = async () => {
    setBusy(true);
    try {
      const ext = format === "json" ? "json" : "txt";
      const path = await save({
        title: t.exportDo,
        defaultPath: `${props.profile.name}-cookies.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      });
      if (!path) return;
      const content = await api.exportCookies(props.profile.id, format);
      await api.writeTextFile(path, content);
      message.success(t.cookieExported);
      props.onClose();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doImport = async () => {
    setBusy(true);
    try {
      const path = await open({
        title: t.importDo,
        multiple: false,
        filters: [
          { name: "Cookies", extensions: ["json", "txt"] },
        ],
      });
      if (!path) return;
      const content = await api.readTextFile(path);
      const count = await api.importCookies(props.profile.id, format, content);
      message.success(t.cookieImported.replace("{n}", String(count)));
      props.onClose();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={props.open}
      title={`${t.cookies}: ${props.profile.name}`}
      onCancel={props.onClose}
      footer={null}
      width={520}
    >
      <Alert style={{ marginBottom: 16 }} type="info" showIcon message={t.cookieHint} />
      <Space direction="vertical" style={{ width: "100%" }} size="large">
        <Radio.Group
          value={format}
          onChange={(e) => setFormat(e.target.value)}
          options={[
            { label: "JSON (Playwright)", value: "json" },
            { label: "Netscape cookies.txt", value: "netscape" },
          ]}
          optionType="button"
        />
        <Space>
          <Button type="primary" icon={<DownloadOutlined />} loading={busy} onClick={doExport}>
            {t.exportDo}
          </Button>
          <Button icon={<UploadOutlined />} loading={busy} onClick={doImport}>
            {t.importDo}
          </Button>
        </Space>
      </Space>
    </Modal>
  );
}
