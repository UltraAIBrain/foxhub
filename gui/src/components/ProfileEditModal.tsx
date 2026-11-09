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
  Typography,
} from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { api } from "../api";
import { useT } from "../i18n";
import type { Profile, ProxyCheckResult, WebrtcMode } from "../types";
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
  webrtcMode: WebrtcMode;
  webrtcIpv4?: string;
  proxyKind: Profile["proxy"]["kind"];
  proxyHost?: string;
  proxyPort?: number;
  proxyUsername?: string;
  proxyPassword?: string;
  automation: boolean;
}

/** Host values used to prefill a new profile, so nothing starts as a blank
 * "auto" the user cannot see the meaning of. */
function hostDefaults(): Partial<FormValues> {
  const ua = navigator.userAgent;
  const os = /Mac/i.test(ua) ? "macos" : /Win/i.test(ua) ? "windows" : "linux";
  const locale = navigator.language || "en-US";
  let timezone: string | undefined;
  try {
    timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    timezone = undefined;
  }
  const screen = `${window.screen.width}x${window.screen.height}`;
  return {
    os,
    locale,
    timezone,
    screen: SCREEN_PRESETS.some((s) => s.value === screen) ? screen : undefined,
  };
}

function parseSize(value: string | undefined): { width: number; height: number } | null {
  if (!value) return null;
  const [w, h] = value.split("x").map((n) => parseInt(n, 10));
  if (!w || !h) return null;
  return { width: w, height: h };
}

/** Fields that describe the presented machine; changing any of them requires
 * a fresh identity draw. */
function identitySignature(p: Profile): string {
  return JSON.stringify({
    os: p.os,
    screen: p.screen,
    window: p.window,
    locale: p.locale,
    timezone: p.timezone,
    geoip: p.geoip,
    webrtcMode: p.webrtcMode,
    webrtcIpv4: p.webrtcIpv4,
    proxy: p.proxy,
  });
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
    webrtcMode: p.webrtcMode || "auto",
    webrtcIpv4: p.webrtcIpv4 || undefined,
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
  /** The stored profile changed; refresh the list. Does not close the modal. */
  onChanged: () => void;
}) {
  const t = useT();
  const [form] = Form.useForm<FormValues>();
  const [saving, setSaving] = useState(false);
  /** The profile as last written to disk. A profile created by this modal is
   * kept here so a retry updates it instead of creating a second one. */
  const [stored, setStored] = useState<Profile | null>(null);
  const [checking, setChecking] = useState(false);
  const [proxyResult, setProxyResult] = useState<ProxyCheckResult | null>(null);
  const [summary, setSummary] = useState<Profile["summary"]>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const proxyKind = Form.useWatch("proxyKind", form);
  const geoipOn = Form.useWatch("geoip", form);
  const webrtcMode = Form.useWatch("webrtcMode", form);

  useEffect(() => {
    if (!props.open) return;
    setStored(props.profile);
    if (props.profile) {
      form.setFieldsValue(toFormValues(props.profile));
      setSummary(props.profile.summary ?? null);
    } else {
      // New profile: start from the host, not from blank auto fields.
      form.setFieldsValue({ ...toFormValues(emptyProfile()), ...hostDefaults() });
      setSummary(null);
    }
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
    webrtcMode: values.webrtcMode,
    webrtcIpv4: values.webrtcMode === "custom" ? values.webrtcIpv4 ?? "" : "",
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
    // buildProfile would silently drop a half-filled pair, so the window would
    // fall back to "auto" without telling anyone.
    if (!!values.windowWidth !== !!values.windowHeight) {
      message.warning(t.windowPairRequired);
      return;
    }
    setSaving(true);
    try {
      const isNew = !stored?.id;
      const saved = await api.saveProfile(buildProfile(stored ?? emptyProfile(), values));
      setStored(saved);
      // The stored identity is frozen; anything that describes the machine
      // must be drawn again, or the form would claim values the browser does
      // not present.
      const needsRegen =
        isNew ||
        (stored ? identitySignature(saved) !== identitySignature(stored) : false) ||
        (saved.fingerprintMode === "fixed" && stored?.fingerprintMode === "random");
      let prepared = true;
      if (needsRegen) {
        message.loading({ content: t.generating, key: "prepare", duration: 0 });
        try {
          const result = await api.prepareProfile(saved.id);
          setSummary(result.profile.summary ?? null);
          setWarnings(result.warnings);
          message.success({ content: t.save, key: "prepare" });
        } catch (e) {
          // The profile is on disk but has no identity. Closing the modal here
          // would hide the only explanation of why it shows "no fingerprint".
          prepared = false;
          message.error({ content: String(e), key: "prepare" });
        }
      }
      props.onChanged();
      if (prepared) props.onClose();
    } catch (e) {
      message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleRegenerate = async () => {
    const id = stored?.id;
    if (!id) {
      message.warning(t.saveFirst);
      return;
    }
    setSaving(true);
    message.loading({ content: t.generating, key: "prepare", duration: 0 });
    try {
      const result = await api.prepareProfile(id);
      setSummary(result.profile.summary ?? null);
      setWarnings(result.warnings);
      message.success({ content: t.regenerated, key: "prepare" });
      props.onChanged();
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
      title={stored ? `${t.edit}: ${stored.name}` : t.newProfile}
      width={720}
      onCancel={props.onClose}
      // The fingerprint tab is taller than an 820px window; without a bounded,
      // scrollable body the footer ends up below the screen and cannot be reached.
      styles={{ body: { maxHeight: "62vh", overflowY: "auto", paddingRight: 8 } }}
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
                  <Form.Item
                    name="fingerprintMode"
                    label={t.fieldMode}
                    extra={
                      <span style={{ fontSize: 12 }}>
                        {t.modeFixedHint} {t.modeRandomHint}
                      </span>
                    }
                  >
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
                        placeholder={t.auto}
                        options={[
                          { label: "Windows", value: "windows" },
                          { label: "macOS", value: "macos" },
                          { label: "Linux", value: "linux" },
                        ]}
                      />
                    </Form.Item>
                    <Form.Item name="screen" label={t.fieldScreen} extra={t.fieldScreenHint}>
                      <Select
                        style={{ width: 180 }}
                        allowClear
                        placeholder={`${t.auto} (${window.screen.width}×${window.screen.height})`}
                        options={SCREEN_PRESETS}
                      />
                    </Form.Item>
                    <Form.Item label={t.fieldWindow} extra={t.windowHint}>
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
                      <Select
                        style={{ width: 170 }}
                        allowClear
                        placeholder={t.auto}
                        options={LOCALE_OPTIONS}
                        showSearch
                      />
                    </Form.Item>
                    <Form.Item name="timezone" label={t.fieldTimezone}>
                      <Select
                        style={{ width: 230 }}
                        allowClear
                        placeholder={
                          Intl.DateTimeFormat().resolvedOptions().timeZone || t.auto
                        }
                        options={TIMEZONE_OPTIONS}
                        showSearch
                      />
                    </Form.Item>
                  </Space>
                  <Form.Item name="geoip" label={t.fieldGeoip} valuePropName="checked" extra={t.geoipHint}>
                    <Switch />
                  </Form.Item>
                  <Form.Item name="webrtcMode" label={t.webrtcMode}>
                    <Radio.Group
                      optionType="button"
                      options={[
                        { label: t.webrtcAuto, value: "auto" },
                        { label: t.webrtcBlock, value: "block" },
                        { label: t.webrtcCustom, value: "custom" },
                      ]}
                    />
                  </Form.Item>
                  {webrtcMode === "auto" && (
                    <Alert style={{ marginBottom: 12 }} type="info" showIcon message={t.webrtcAutoHint} />
                  )}
                  {webrtcMode === "block" && (
                    <Alert style={{ marginBottom: 12 }} type="warning" showIcon message={t.webrtcBlockHint} />
                  )}
                  {webrtcMode === "custom" && (
                    <>
                      <Form.Item
                        name="webrtcIpv4"
                        label={t.webrtcIpv4}
                        rules={[{ required: true, pattern: /^\d{1,3}(\.\d{1,3}){3}$/ }]}
                        extra={t.webrtcCustomHint}
                      >
                        <Input style={{ width: 220 }} placeholder="203.0.113.10" />
                      </Form.Item>
                    </>
                  )}
                  <Space direction="vertical" style={{ width: "100%" }} size="middle">
                    <Space>
                      <Button icon={<ReloadOutlined />} loading={saving} onClick={handleRegenerate}>
                        {t.regenerate}
                      </Button>
                      {summary && (
                        <Typography.Text type="secondary" style={{ fontSize: 12 }} copyable>
                          {t.identityId}: {summary.identityId}
                        </Typography.Text>
                      )}
                    </Space>
                    {warnings.length > 0 && (
                      <Alert
                        type="warning"
                        showIcon
                        message={warnings.join("\n")}
                        style={{ whiteSpace: "pre-wrap" }}
                      />
                    )}
                    {summary ? (
                      <div className="camfox-fingerprint-preview">
                        <Descriptions size="small" column={2} title={t.fingerprintInfo}>
                          <Descriptions.Item label="UA" span={2}>
                            <Typography.Text style={{ fontSize: 12 }} copyable>
                              {summary.ua ?? "—"}
                            </Typography.Text>
                          </Descriptions.Item>
                          <Descriptions.Item label="Platform">{summary.platform ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label={t.fieldOs}>{summary.oscpu ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label={t.fieldScreen}>
                            {summary.screen ? `${summary.screen[0]}×${summary.screen[1]}` : "—"}
                            {summary.availScreen?.[1] ? ` (可用 ${summary.availScreen[1]})` : ""}
                          </Descriptions.Item>
                          <Descriptions.Item label={t.fieldColorDepth}>{summary.colorDepth ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label={t.fieldTimezone}>{summary.timezone ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label={t.fieldLocale}>{summary.locale ?? "—"}</Descriptions.Item>
                          <Descriptions.Item label="Cores / DPR">
                            {summary.hardwareConcurrency ?? "—"} / {summary.devicePixelRatio ?? "—"}
                          </Descriptions.Item>
                          <Descriptions.Item label={t.webrtcFollows}>
                            {summary.webrtcIpv4 ?? t.none}
                          </Descriptions.Item>
                          <Descriptions.Item label={t.gpu} span={2}>
                            <Typography.Text style={{ fontSize: 12 }}>
                              {summary.gpuRenderer ?? "—"}
                            </Typography.Text>
                            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                              {" "}
                              ({summary.gpuVendor ?? "—"})
                            </Typography.Text>
                          </Descriptions.Item>
                          <Descriptions.Item label={t.fonts}>{summary.fontsCount ?? 0}</Descriptions.Item>
                          <Descriptions.Item label={t.voices}>{summary.voicesCount ?? 0}</Descriptions.Item>
                          <Descriptions.Item label={t.mediaDevices} span={2}>
                            {summary.mediaDevices?.join(" / ") ?? "—"} <Typography.Text type="secondary">({t.mediaUnits})</Typography.Text>
                          </Descriptions.Item>
                        </Descriptions>
                        <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginTop: 8, marginBottom: 0 }}>
                          {summary.canvasCovered ? t.canvasCoveredNote : t.canvasNote}
                        </Typography.Paragraph>
                      </div>
                    ) : (
                      <Alert type="info" showIcon message={t.identityIdHint} />
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
                      {!geoipOn && (
                        <Alert
                          style={{ marginBottom: 12 }}
                          type="info"
                          showIcon
                          message={t.proxyGeoipHint}
                        />
                      )}
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
