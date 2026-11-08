import { useEffect, useState } from "react";
import { Button, Descriptions, Form, Input, message, Select, Switch } from "antd";
import { api } from "../api";
import { dictionaries, useT } from "../i18n";
import type { Settings } from "../types";

export function SettingsPage(props: { language: string; onLanguage: (lang: string) => void }) {
  const t = useT();
  const [form] = Form.useForm<Settings>();
  const [dataDir, setDataDir] = useState("");
  const [saving, setSaving] = useState(false);

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
        <Button type="primary" htmlType="submit" loading={saving}>
          {t.save}
        </Button>
      </Form>
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
