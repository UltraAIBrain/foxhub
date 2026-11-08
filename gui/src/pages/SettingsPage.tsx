import { useEffect, useState } from "react";
import { Alert, Button, Descriptions, Form, Input, message, Select, Space, Spin, Switch } from "antd";
import { api } from "../api";
import { dictionaries, useT } from "../i18n";
import type { Settings } from "../types";

interface DiagResult {
  sidecarMode: string;
  sidecarDir: string | null;
  python: string | null;
  kernelVersion: string | null;
  kernelPath: string | null;
  error: string | null;
}

export function SettingsPage(props: { language: string; onLanguage: (lang: string) => void }) {
  const t = useT();
  const [form] = Form.useForm<Settings>();
  const [dataDir, setDataDir] = useState("");
  const [saving, setSaving] = useState(false);
  const [diag, setDiag] = useState<DiagResult | null>(null);
  const [diagBusy, setDiagBusy] = useState(false);

  useEffect(() => {
    api.getSettings().then((s) => form.setFieldsValue(s)).catch(() => undefined);
    api.dataDir().then(setDataDir).catch(() => undefined);
  }, [form]);

  const onSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      await api.saveSettings(values);
      props.onLanguage(values.language);
      message.success(t.settingsSaved);
    } catch (e) {
      message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  const onDiagnose = async () => {
    setDiagBusy(true);
    try {
      setDiag(await api.diagnose());
    } catch (e) {
      message.error(String(e));
    } finally {
      setDiagBusy(false);
    }
  };

  const diagLines = (d: DiagResult) =>
    [
      `${t.diagSidecar}: ${d.sidecarMode}${d.sidecarDir ? ` (${d.sidecarDir})` : ""}`,
      d.python ? `${t.diagPython}: ${d.python}` : null,
      `${t.diagKernel}: ${d.kernelVersion ?? "—"}${d.kernelPath ? ` (${d.kernelPath})` : ""}`,
      d.error ? `${t.diagFail}: ${d.error}` : null,
    ]
      .filter(Boolean)
      .join("\n");

  return (
    <div style={{ maxWidth: 640 }}>
      <Form form={form} layout="vertical" onFinish={onSave}>
        <Form.Item name="pythonPath" label={t.settingsPython} extra="python / python3 / C:\\...\\python.exe">
          <Input placeholder="python" />
        </Form.Item>
        <Form.Item name="sidecarPath" label={t.settingsSidecar} extra="…/camoufox/gui/sidecar">
          <Input placeholder="(auto)" />
        </Form.Item>
        <Form.Item name="kernelExecutable" label={t.settingsKernelExec}>
          <Input placeholder="(managed kernel)" />
        </Form.Item>
        <Form.Item name="language" label={t.settingsLanguage}>
          <Select
            options={Object.keys(dictionaries).map((k) => ({ label: k === "zh-CN" ? "中文" : "English", value: k }))}
          />
        </Form.Item>
        <Form.Item name="closeBrowsersOnExit" label={t.settingsCloseOnExit} valuePropName="checked">
          <Switch />
        </Form.Item>
        <Space>
          <Button type="primary" htmlType="submit" loading={saving}>
            {t.save}
          </Button>
          <Button loading={diagBusy} onClick={onDiagnose}>
            {t.diagnose}
          </Button>
        </Space>
      </Form>
      {diagBusy && <Spin style={{ marginTop: 16 }} />}
      {diag && !diagBusy && (
        <Alert
          style={{ marginTop: 16 }}
          type={diag.error ? "error" : "success"}
          showIcon
          message={diag.error ? t.diagFail : t.settingsSaved}
          description={<pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{diagLines(diag)}</pre>}
        />
      )}
      <Descriptions
        style={{ marginTop: 32 }}
        column={1}
        size="small"
        bordered
        items={[{ key: "dir", label: t.settingsDataDir, children: dataDir || "…" }]}
      />
    </div>
  );
}
