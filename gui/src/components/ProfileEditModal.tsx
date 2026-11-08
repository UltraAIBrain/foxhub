import { useEffect, useState } from "react";
import {
  Alert,
  AutoComplete,
  Button,
  Descriptions,
  Form,
  Input,
  InputNumber,
  message,
  Modal,
  Radio,
  Select,
  Space,
  Switch,
  Tabs,
} from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { api } from "../api";
import { useT } from "../i18n";
import type { Profile, ProxyCheckResult } from "../types";
import { emptyProfile } from "../types";

const SCREEN_PRESETS: { label: string; value: string }[] = [
  { label: "1920 × 1080", value: "1920x1080" },
  { label: "1680 × 1050", value: "1680x1050" },
  { label: "1600 × 900", value: "1600x900" },
  { label: "1536 × 864", value: "1536x864" },
  { label: "1440 × 900", value: "1440x900" },
  { label: "1366 × 768", value: "1366x768" },
];

const LOCALE_OPTIONS = ["en-US", "en-GB", "zh-CN", "zh-TW", "ja-JP", "ko-KR", "de-DE", "fr-FR", "es-ES", "pt-BR", "ru-RU"].map(
  (v) => ({ label: v, value: v }),
);

const TIMEZONE_OPTIONS = [
  "America/New_York",
  "America/Chicago",
  "America/Denver",
  "America/Los_Angeles",
  "America/Sao_Paulo",
  "Europe/London",
  "Europe/Paris",
  "Europe/Berlin",
  "Europe/Moscow",
  "Asia/Shanghai",
  "Asia/Hong_Kong",
  "Asia/Taipei",
  "Asia/Singapore",
  "Asia/Tokyo",
  "Asia/Seoul",
  "Australia/Sydney",
].map((v) => ({ label: v, value: v }));

interface FormValues {
  name: string;
  group: string;
  notes: string;
  fingerprintMode: "fixed" | "random";
  os: string | undefined;
  screen: string | undefined;
  windowWidth?: number;
  windowHeight?: number;
  locale?: string;
  timezone?: string;
  geoip: boolean;
  proxyKind: Profile["proxy"]["kind"];
  proxyHost?: string;
  proxyPort?: number;
  proxyUsername?: string;
  proxyPassword?: string;
  automation: boolean;
}

function parseSize(value: string | undefined): { width: number; height: number } | null {
  if (!value) return null;
  const [w, h] = value.split("x").map((n) => parseInt(n, 10));
  if (!w || !h) return null;
  return { width: w, height: h };
}

function toFormValues(p: Profile): FormValues {
  return {
    name: p.name,
    group: p.group,
    notes: p.notes,
    fingerprintMode: p.fingerprintMode,
    os: p.os ?? undefined,
    screen: p.screen ? `${p.screen.width}x${p.screen.height}` : undefined,
    windowWidth: p.window?.width,
    windowHeight: p.window?.height,
    locale: p.locale ?? undefined,
    timezone: p.timezone ?? undefined,
    geoip: p.geoip,
    proxyKind: p.proxy.kind,
    proxyHost: p.proxy.host || undefined,
    proxyPort: p.proxy.port || undefined,
    proxyUsername: p.proxy.username || undefined,
    proxyPassword: p.proxy.password || undefined,
    automation: p.automation,
  };
}

export function ProfileEditModal(props: {
  open: boolean;
  profile: Profile | null; // null = create
  groups: string[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const t = useT();
  const [form] = Form.useForm<FormValues>();
  const [saving, setSaving] = useState(false);
  const [checking, setChecking] = useState(false);
  const [proxyResult, setProxyResult] = useState<ProxyCheckResult | null>(null);
  const [summary, setSummary] = useState<Profile["summary"]>(null);
  const proxyKind = Form.useWatch("proxyKind", form);

  useEffect(() => {
    if (!props.open) return;
    const base = props.profile ?? emptyProfile();
    form.setFieldsValue(toFormValues(base));
    setSummary(base.summary ?? null);
    setProxyResult(null);
  }, [props.open, props.profile, form]);

  const buildProfile = (existing: Profile, values: FormValues): Profile => ({
    ...existing,
    name: values.name.trim(),
    group: values.group?.trim() ?? "",
    notes: values.notes ?? "",
    fingerprintMode: values.fingerprintMode,
    os: values.os ?? null,
    screen: parseSize(values.screen),
    window:
      values.windowWidth && values.windowHeight
        ? { width: values.windowWidth, height: values.windowHeight }
        : null,
    locale: values.locale ?? null,
    timezone: values.timezone ?? null,
    geoip: values.geoip,
    automation: values.automation,
    proxy: {
      kind: values.proxyKind,
      host: values.proxyHost ?? "",
      port: values.proxyPort ?? 0,
      username: values.proxyUsername ?? "",
      password: values.proxyPassword ?? "",
    },
  });

  const currentProxy = (values: FormValues): Profile["proxy"] => ({
    kind: values.proxyKind,
    host: values.proxyHost ?? "",
    port: values.proxyPort ?? 0,
    username: values.proxyUsername ?? "",
    password: values.proxyPassword ?? "",
  });

  const handleCheckProxy = async () => {
    const values = await form.validateFields(["proxyKind", "proxyHost", "proxyPort", "proxyUsername", "proxyPassword"]);
    setChecking(true);
    setProxyResult(null);
    try {
      const result = await api.checkProxy(currentProxy(values));
      setProxyResult(result);
    } catch (e) {
      setProxyResult({ error: String(e) });
    } finally {
      setChecking(false);
    }
  };

  const handleSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      const isNew = !props.profile?.id;
      const saved = await api.saveProfile(buildProfile(props.profile ?? emptyProfile(), values));
      // A new profile draws its identity immediately so it is launch-ready.
      if (isNew) {
        message.loading({ content: t.generating, key: "prepare", duration: 0 });
        try {
          const prepared = await api.prepareProfile(saved.id);
          setSummary(prepared.summary ?? null);
          message.success({ content: t.save, key: "prepare" });
        } catch (e) {
          message.error({ content: String(e), key: "prepare" });
        }
      }
      props.onSaved();
      if (!isNew) props.onClose();
    } catch (e) {
      message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleRegenerate = async () => {
    if (!props.profile?.id) {
      message.warning(t.save);
      return;
    }
    setSaving(true);
    message.loading({ content: t.generating, key: "prepare", duration: 0 });
    try {
      const prepared = await api.prepareProfile(props.profile.id);
      setSummary(prepared.summary ?? null);
      message.success({ content: t.regenerate, key: "prepare" });
      props.onSaved();
    } catch (e) {
      message.error({ content: String(e), key: "prepare" });
    } finally {
      setSaving(false);
    }
  };

  const hasProxy = proxyKind === "http" || proxyKind === "socks5";

  return (
    <Modal
      open={props.open}
      title={props.profile ? `${t.edit}: ${props.profile.name}` : t.newProfile}
      width={720}
      onCancel={props.onClose}
      footer={[
        <Button key="cancel" onClick={props.onClose}>
          {t.cancel}
        </Button>,
        <Button key="save" type="primary" loading={saving} onClick={handleSave}>
          {t.save}
        </Button>,
      ]}
    >
      <Form form={form} layout="vertical">
        {/* forceRender on every panel: validateFields only sees mounted
            Form.Items, so a save from the first tab would otherwise send
            fingerprint/proxy/automation values as undefined. */}
        <Tabs
          items={[
            {
              key: "basic",
              label: t.tabBasic,
              forceRender: true,
              children: (
                <>
                  <Form.Item name="name" label={t.fieldName} rules={[{ required: true }]}>
                    <Input />
                  </Form.Item>
                  <Form.Item name="group" label={t.fieldGroup}>
                    <AutoComplete
                      options={props.groups.map((g) => ({ value: g }))}
                      allowClear
                    />
                  </Form.Item>
                  <Form.Item name="notes" label={t.fieldNotes}>
                    <Input.TextArea rows={2} />
                  </Form.Item>
                </>
              ),
            },
            {
              key: "fingerprint",
              label: t.tabFingerprint,
              forceRender: true,
              children: (
                <>
                  <Form.Item name="fingerprintMode" label={t.fieldMode}>
                    <Radio.Group
                      options={[
                        { label: t.modeFixed, value: "fixed" },
                        { label: t.modeRandom, value: "random" },
                      ]}
                      optionType="button"
                    />
                  </Form.Item>
                  <Space size="large" wrap>
                    <Form.Item name="os" label={t.fieldOs}>
                      <Select
                        style={{ width: 160 }}
                        allowClear
                        placeholder="auto"
                        options={[
                          { label: "Windows", value: "windows" },
                          { label: "macOS", value: "macos" },
                          { label: "Linux", value: "linux" },
                        ]}
                      />
                    </Form.Item>
                    <Form.Item name="screen" label={t.fieldScreen}>
                      <Select style={{ width: 180 }} allowClear placeholder="auto" options={SCREEN_PRESETS} />
                    </Form.Item>
                    <Form.Item label={t.fieldWindow} style={{ marginBottom: 0 }}>
                      <Space.Compact>
                        <Form.Item name="windowWidth" noStyle>
                          <InputNumber placeholder="width" min={400} max={3840} style={{ width: 110 }} />
                        </Form.Item>
                        <Form.Item name="windowHeight" noStyle>
                          <InputNumber placeholder="height" min={300} max={2160} style={{ width: 110 }} />
                        </Form.Item>
                      </Space.Compact>
                    </Form.Item>
                  </Space>
                  <Space size="large" wrap>
                    <Form.Item name="locale" label={t.fieldLocale}>
                      <Select style={{ width: 160 }} allowClear placeholder="auto" options={LOCALE_OPTIONS} showSearch />
                    </Form.Item>
                    <Form.Item name="timezone" label={t.fieldTimezone}>
                      <Select
                        style={{ width: 220 }}
                        allowClear
                        placeholder="auto"
                        options={TIMEZONE_OPTIONS}
                        showSearch
                      />
                    </Form.Item>
                  </Space>
                  <Form.Item name="geoip" label={t.fieldGeoip} valuePropName="checked">
                    <Switch disabled={!hasProxy} />
                  </Form.Item>
                  <Space direction="vertical" style={{ width: "100%" }}>
                    <Button icon={<ReloadOutlined />} loading={saving} onClick={handleRegenerate}>
                      {t.regenerate}
                    </Button>
                    {summary && (
                      <div className="camfox-fingerprint-preview">
                        <Descriptions size="small" column={2} title={t.fingerprintInfo}>
                          <Descriptions.Item label="UA" span={2}>
                            {summary.ua ?? "—"}
                          </Descriptions.Item>
                          <Descriptions.Item label="Platform">{summary.platform ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label={t.fieldScreen}>
                            {summary.screen ? `${summary.screen[0]}×${summary.screen[1]}` : "—"}
                          </Descriptions.Item>
                          <Descriptions.Item label={t.fieldTimezone}>{summary.timezone ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label={t.fieldLocale}>{summary.locale ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label="IP">{summary.ip ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label="Cores">{summary.hardwareConcurrency ?? "—"}</Descriptions.Item>
                        </Descriptions>
                      </div>
                    )}
                  </Space>
                </>
              ),
            },
            {
              key: "proxy",
              label: t.tabProxy,
              forceRender: true,
              children: (
                <>
                  <Form.Item name="proxyKind" label={t.proxyKind}>
                    <Radio.Group
                      options={[
                        { label: t.proxyNone, value: "none" },
                        { label: t.proxyHttp, value: "http" },
                        { label: t.proxySocks5, value: "socks5" },
                      ]}
                      optionType="button"
                    />
                  </Form.Item>
                  {hasProxy && (
                    <>
                      <Space size="large" wrap>
                        <Form.Item name="proxyHost" label={t.proxyHost} rules={[{ required: true }]}>
                          <Input style={{ width: 220 }} placeholder="proxy.example.com" />
                        </Form.Item>
                        <Form.Item name="proxyPort" label={t.proxyPort} rules={[{ required: true }]}>
                          <InputNumber min={1} max={65535} style={{ width: 120 }} />
                        </Form.Item>
                      </Space>
                      <Space size="large" wrap>
                        <Form.Item name="proxyUsername" label={t.proxyUser}>
                          <Input autoComplete="off" style={{ width: 200 }} />
                        </Form.Item>
                        <Form.Item name="proxyPassword" label={t.proxyPass}>
                          <Input.Password autoComplete="new-password" style={{ width: 200 }} />
                        </Form.Item>
                      </Space>
                      <Button loading={checking} onClick={handleCheckProxy}>
                        {t.checkProxy}
                      </Button>
                      {proxyResult && (
                        <Alert
                          style={{ marginTop: 12 }}
                          type={proxyResult.error ? "error" : "success"}
                          showIcon
                          message={
                            proxyResult.error
                              ? proxyResult.error
                              : `${t.proxyOk}: ${proxyResult.ip} · ${proxyResult.timezone ?? t.proxyGeoipError}${
                                  proxyResult.locale ? ` · ${proxyResult.locale}` : ""
                                }`
                          }
                        />
                      )}
                    </>
                  )}
                </>
              ),
            },
            {
              key: "automation",
              label: t.tabAutomation,
              forceRender: true,
              children: (
                <>
                  <Form.Item name="automation" label={t.automationEnable} valuePropName="checked">
                    <Switch />
                  </Form.Item>
                  <Alert type="info" showIcon message={t.automationHint} />
                </>
              ),
            },
          ]}
        />
      </Form>
    </Modal>
  );
}
