import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "@/bindings";
import type { McpTool, SourceStatus, VoiceSource } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Radio } from "../../ui/Radio";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { Spinner } from "../../ui/Spinner";

const SOURCES: VoiceSource[] = ["mcp", "file", "built_in"];
const SOURCE_KEY: Record<VoiceSource, string> = {
  mcp: "mcp",
  file: "file",
  built_in: "builtIn",
};
const NONE = "__none__";

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

export const VoiceSourceSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, refreshSettings, isUpdating } =
    useSettings();
  const [status, setStatus] = useState<SourceStatus | null>(null);
  const [tools, setTools] = useState<McpTool[] | null>(null);
  const [urlDraft, setUrlDraft] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const guidePath = (getSetting("voice_guide_path") ?? null) as string | null;
  const wordsPath = (getSetting("voice_words_path") ?? null) as string | null;
  const savedUrl = (getSetting("voice_mcp_url") ?? null) as string | null;
  const guideTool = (getSetting("voice_mcp_guide_tool") ?? null) as
    | string
    | null;
  const wordsTool = (getSetting("voice_mcp_words_tool") ?? null) as
    | string
    | null;
  const selected: VoiceSource =
    (getSetting("voice_source") as VoiceSource | null | undefined) ??
    (guidePath ? "file" : "built_in");

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await commands.voiceSourceStatus());
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    loadStatus();
  }, [loadStatus]);

  const connected = selected === "mcp" && !!savedUrl && !!status?.server_name;
  const needsSignIn = !!savedUrl && !!status?.needs_sign_in;

  // Tools are only needed to offer a manual pick when the server doesn't
  // use the standard names.
  useEffect(() => {
    if (selected !== "mcp" || !savedUrl || !connected) {
      setTools(null);
      return;
    }
    let cancelled = false;
    commands
      .voiceSourceTools()
      .then((result) => {
        if (!cancelled) setTools(result.status === "ok" ? result.data : null);
      })
      .catch(() => {
        if (!cancelled) setTools(null);
      });
    return () => {
      cancelled = true;
    };
  }, [selected, savedUrl, connected]);

  const flash = (text: string) => {
    setMessage(text);
    window.setTimeout(() => setMessage(null), 6000);
  };

  const connect = async (url: string) => {
    setConnecting(true);
    setMessage(null);
    const result = await commands.voiceSourceConnect(url);
    setConnecting(false);
    if (result.status === "error") {
      flash(result.error);
    } else {
      setStatus(result.data);
    }
    await refreshSettings();
    await loadStatus();
  };

  const disconnect = async () => {
    const result = await commands.voiceSourceDisconnect();
    if (result.status === "error") {
      flash(result.error);
      return;
    }
    setStatus(null);
    setTools(null);
    setUrlDraft("");
    await refreshSettings();
    await loadStatus();
  };

  const syncNow = async () => {
    setSyncing(true);
    const result = await commands.voiceSourceSync();
    if (result.status === "ok") setStatus(result.data);
    else flash(result.error);
    setSyncing(false);
  };

  const chooseFile = async (kind: "guide" | "words") => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Text", extensions: ["md", "txt", "markdown"] }],
    });
    if (typeof picked !== "string") return;
    if (kind === "guide") {
      const result =
        await commands.changeTransformVoiceGuidePathSetting(picked);
      if (result.status === "error") {
        flash(
          t("settings.transform.voiceGuide.chooseFailed", {
            error: result.error,
          }),
        );
        return;
      }
      await updateSetting("voice_guide_path", picked);
    } else {
      await updateSetting("voice_words_path", picked);
    }
  };

  const saveTools = async (guide: string | null, words: string | null) => {
    const result = await commands.changeVoiceMcpToolsSetting(guide, words);
    if (result.status === "error") {
      flash(result.error);
      return;
    }
    await refreshSettings();
    await syncNow();
  };

  const relativeTime = (seconds: number | null) => {
    if (!seconds) return t("settings.transform.voiceSource.never");
    const minutes = Math.max(0, Math.floor((Date.now() / 1000 - seconds) / 60));
    if (minutes < 1) return t("settings.transform.voiceSource.justNow");
    if (minutes < 60)
      return t("settings.transform.voiceSource.minutesAgo", {
        count: minutes,
      });
    return t("settings.transform.voiceSource.hoursAgo", {
      count: Math.floor(minutes / 60),
    });
  };

  const usable = (tools ?? []).filter((tool) => tool.required_args === 0);
  const toolOptions = [
    { value: NONE, label: t("settings.transform.voiceSource.noTool") },
    ...usable.map((tool) => ({ value: tool.name, label: tool.name })),
  ];
  const standardFound =
    (tools ?? []).some((tool) => tool.name === "get_voice_guide") &&
    (tools ?? []).some((tool) => tool.name === "get_vocabulary");
  const showToolPicker = connected && tools !== null && !standardFound;

  const pathRow = (
    titleKey: string,
    path: string | null,
    kind: "guide" | "words",
  ) => (
    <SettingContainer
      title={t(titleKey)}
      description={
        path ? fileName(path) : t("settings.transform.voiceSource.noFile")
      }
      descriptionMode="inline"
      grouped={true}
    >
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={() => chooseFile(kind)}
        >
          {t("settings.transform.voiceGuide.choose")}
        </Button>
        {path && kind === "words" && (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => updateSetting("voice_words_path", null)}
          >
            {t("settings.transform.voiceSource.clear")}
          </Button>
        )}
      </div>
    </SettingContainer>
  );

  return (
    <SettingsGroup title={t("settings.transform.voiceSource.title")}>
      <div
        role="radiogroup"
        aria-label={t("settings.transform.voiceSource.title")}
      >
        {SOURCES.map((source) => {
          const checked = source === selected;
          const key = SOURCE_KEY[source];
          return (
            <button
              key={source}
              type="button"
              role="radio"
              aria-checked={checked}
              disabled={isUpdating("voice_source")}
              onClick={() => updateSetting("voice_source", source)}
              className="flex w-full cursor-pointer items-center justify-between gap-4 px-4 p-2 text-start hover:bg-wash disabled:cursor-default"
            >
              <span>
                <span className="block text-sm font-medium">
                  {t(`settings.transform.voiceSource.options.${key}.name`)}
                </span>
                <span className="block text-sm text-ink-3">
                  {t(
                    `settings.transform.voiceSource.options.${key}.description`,
                  )}
                </span>
              </span>
              <Radio checked={checked} />
            </button>
          );
        })}
      </div>

      {selected === "mcp" && (
        <>
          <SettingContainer
            title={t("settings.transform.voiceSource.serverAddress")}
            description={
              savedUrl ? "" : t("settings.transform.voiceSource.hint")
            }
            descriptionMode="inline"
            grouped={true}
          >
            <div className="flex items-center gap-2">
              {savedUrl ? (
                <>
                  <Input
                    variant="compact"
                    type="text"
                    value={savedUrl}
                    readOnly
                    className="w-64"
                  />
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={connecting}
                    onClick={disconnect}
                  >
                    {t("settings.transform.voiceSource.disconnect")}
                  </Button>
                </>
              ) : (
                <>
                  <Input
                    variant="compact"
                    type="text"
                    value={urlDraft}
                    spellCheck={false}
                    autoCapitalize="off"
                    autoCorrect="off"
                    placeholder={t(
                      "settings.transform.voiceSource.addressPlaceholder",
                    )}
                    disabled={connecting}
                    onChange={(e) => setUrlDraft(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && urlDraft.trim())
                        connect(urlDraft);
                    }}
                    className="w-64"
                  />
                  <Button
                    type="button"
                    variant="primary"
                    size="sm"
                    disabled={connecting || !urlDraft.trim()}
                    onClick={() => connect(urlDraft)}
                  >
                    {t("settings.transform.voiceSource.connect")}
                  </Button>
                </>
              )}
            </div>
          </SettingContainer>

          {connecting && (
            <div
              role="status"
              className="flex items-center gap-2 px-4 pb-2 text-sm text-ink-3"
            >
              <Spinner />
              {t("settings.transform.voiceSource.waitingForBrowser")}
            </div>
          )}

          {savedUrl && !connecting && (
            <div className="mx-4 my-2 flex items-center gap-3 rounded-lg border border-line p-3">
              <span
                aria-hidden="true"
                className={`inline-block h-2 w-2 flex-none rounded-full ${
                  needsSignIn
                    ? "bg-warning"
                    : connected
                      ? "bg-green-600"
                      : "bg-ink-3"
                }`}
              />
              <div className="min-w-0 flex-1 text-sm">
                {needsSignIn ? (
                  <span className="font-medium">
                    {t("settings.transform.voiceSource.signInAgain")}
                  </span>
                ) : connected ? (
                  <>
                    <span className="block">
                      {t("settings.transform.voiceSource.connectedTo", {
                        name: status?.server_name,
                      })}
                    </span>
                    <span className="block text-ink-3">
                      {t("settings.transform.voiceSource.guideLine", {
                        version: status?.guide_version ?? "–",
                        time: relativeTime(status?.synced_at ?? null),
                      })}
                    </span>
                  </>
                ) : (
                  <span className="text-ink-3">
                    {status?.error ??
                      t("settings.transform.voiceSource.notSynced")}
                  </span>
                )}
                {status?.error && connected && (
                  <span className="block text-red-600">{status.error}</span>
                )}
              </div>
              {needsSignIn ? (
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  disabled={connecting}
                  onClick={() => connect(savedUrl)}
                >
                  {t("settings.transform.voiceSource.connect")}
                </Button>
              ) : (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={syncing}
                  onClick={syncNow}
                >
                  {t("settings.transform.voiceSource.syncNow")}
                </Button>
              )}
            </div>
          )}

          {showToolPicker && (
            <>
              <SettingContainer
                title={t("settings.transform.voiceSource.guideTool")}
                description={t("settings.transform.voiceSource.toolPickerHint")}
                descriptionMode="inline"
                grouped={true}
              >
                <Select
                  className="w-56"
                  value={guideTool ?? NONE}
                  options={toolOptions}
                  onChange={(v) =>
                    saveTools(v && v !== NONE ? v : null, wordsTool)
                  }
                />
              </SettingContainer>
              <SettingContainer
                title={t("settings.transform.voiceSource.wordsTool")}
                description=""
                descriptionMode="inline"
                grouped={true}
              >
                <Select
                  className="w-56"
                  value={wordsTool ?? NONE}
                  options={toolOptions}
                  onChange={(v) =>
                    saveTools(guideTool, v && v !== NONE ? v : null)
                  }
                />
              </SettingContainer>
            </>
          )}
        </>
      )}

      {selected === "file" && (
        <>
          {pathRow(
            "settings.transform.voiceSource.guideFile",
            guidePath,
            "guide",
          )}
          {pathRow(
            "settings.transform.voiceSource.wordsFile",
            wordsPath,
            "words",
          )}
        </>
      )}

      {selected === "built_in" && (
        <SettingContainer
          title={t("settings.transform.voiceGuide.title")}
          description={t("settings.transform.voiceGuide.description")}
          descriptionMode="inline"
          grouped={true}
        >
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => commands.openVoiceGuide()}
          >
            {t("settings.transform.voiceGuide.button")}
          </Button>
        </SettingContainer>
      )}

      {message && (
        <p role="status" className="px-4 pb-2 text-sm text-red-600">
          {message}
        </p>
      )}
    </SettingsGroup>
  );
};
