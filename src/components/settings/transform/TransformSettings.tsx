import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "@/bindings";
import type { OllamaStatus, TransformEngine, TransformStyle } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Radio } from "../../ui/Radio";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { ShortcutInput } from "../ShortcutInput";

const STYLES: TransformStyle[] = [
  "correct_grammar",
  "formal",
  "friendlier",
  "clearer",
  "shorter",
  "my_voice",
];

const ENGINES: { id: TransformEngine; labelKey: string }[] = [
  { id: "ollama", labelKey: "settings.transform.engine.ollama" },
  {
    id: "apple_intelligence",
    labelKey: "settings.transform.engine.appleIntelligence",
  },
];

const DEFAULT_OLLAMA_MODEL = "qwen2.5:7b";

type EngineStatus =
  | "checking"
  | "ready"
  | "unavailable"
  | "notRunning"
  | "modelMissing";

const OLLAMA_STATUS_KEY: Record<OllamaStatus, EngineStatus> = {
  ready: "ready",
  not_running: "notRunning",
  model_missing: "modelMissing",
};

export const TransformSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [status, setStatus] = useState<EngineStatus>("checking");
  const [syncMessage, setSyncMessage] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncConfigured, setSyncConfigured] = useState(false);

  const engineSetting = (getSetting("transform_engine") ??
    "ollama") as TransformEngine;
  const savedModel = (getSetting("transform_ollama_model") ??
    DEFAULT_OLLAMA_MODEL) as string;
  const [modelDraft, setModelDraft] = useState(savedModel);

  useEffect(() => {
    setModelDraft(savedModel);
  }, [savedModel]);

  useEffect(() => {
    let cancelled = false;
    setStatus("checking");
    const check =
      engineSetting === "ollama"
        ? commands
            .checkOllamaStatus(savedModel)
            .then((result) => OLLAMA_STATUS_KEY[result])
        : commands
            .checkAppleIntelligenceAvailable()
            .then(
              (available): EngineStatus =>
                available ? "ready" : "unavailable",
            );
    check
      .then((result) => {
        if (!cancelled) setStatus(result);
      })
      .catch(() => {
        if (!cancelled)
          setStatus(engineSetting === "ollama" ? "notRunning" : "unavailable");
      });
    return () => {
      cancelled = true;
    };
  }, [engineSetting, savedModel]);

  const guidePath = (getSetting("voice_guide_path") ?? null) as string | null;
  const guideName = guidePath
    ? (guidePath.split(/[\\/]/).pop() ?? guidePath)
    : t("settings.transform.voiceGuide.usingDefault");

  const chooseGuide = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Text", extensions: ["md", "txt", "markdown"] }],
    });
    if (typeof picked !== "string") return;
    const result = await commands.changeTransformVoiceGuidePathSetting(picked);
    if (result.status === "error") {
      setSyncMessage(
        t("settings.transform.voiceGuide.chooseFailed", {
          error: result.error,
        }),
      );
      window.setTimeout(() => setSyncMessage(null), 6000);
      return;
    }
    await updateSetting("voice_guide_path", picked);
  };

  useEffect(() => {
    commands
      .voiceSyncConfigured()
      .then(setSyncConfigured)
      .catch(() => setSyncConfigured(false));
  }, []);

  const commitModel = () => {
    const next = modelDraft.trim();
    if (next && next !== savedModel) {
      updateSetting("transform_ollama_model", next);
    } else {
      setModelDraft(savedModel);
    }
  };

  const syncGuide = async () => {
    setSyncing(true);
    setSyncMessage(t("settings.transform.voiceGuide.syncing"));
    const result = await commands.syncVoiceGuide();
    setSyncMessage(
      result.status === "ok"
        ? t("settings.transform.voiceGuide.synced", { version: result.data })
        : t("settings.transform.voiceGuide.syncFailed", {
            error: result.error,
          }),
    );
    setSyncing(false);
    window.setTimeout(() => setSyncMessage(null), 4000);
  };

  const selected = (getSetting("transform_style") ??
    "correct_grammar") as TransformStyle;

  const engineColor =
    status === "ready"
      ? "text-green-600"
      : status === "checking"
        ? "text-ink-3"
        : "text-red-600";

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.transform.title")}>
        <ShortcutInput shortcutId="transform_selection" grouped={true} />
        <SettingContainer
          title={t("settings.transform.engine.title")}
          description={t("settings.transform.engine.description")}
          descriptionMode="inline"
          grouped={true}
        >
          <div className="flex items-center gap-3">
            <div
              role="radiogroup"
              aria-label={t("settings.transform.engine.title")}
              className="flex gap-1"
            >
              {ENGINES.map((engine) => (
                <Button
                  key={engine.id}
                  type="button"
                  role="radio"
                  aria-checked={engine.id === engineSetting}
                  variant={
                    engine.id === engineSetting ? "primary-soft" : "secondary"
                  }
                  size="sm"
                  disabled={isUpdating("transform_engine")}
                  onClick={() => updateSetting("transform_engine", engine.id)}
                >
                  {t(engine.labelKey)}
                </Button>
              ))}
            </div>
            <span
              className={`flex items-center gap-1.5 text-sm font-medium ${engineColor}`}
            >
              <span
                aria-hidden="true"
                className="inline-block h-2 w-2 rounded-full bg-current"
              />
              {t(`settings.transform.engine.${status}`)}
            </span>
          </div>
        </SettingContainer>
        {engineSetting === "ollama" && (
          <SettingContainer
            title={t("settings.transform.model.title")}
            description={t("settings.transform.model.description")}
            descriptionMode="inline"
            grouped={true}
          >
            <Input
              variant="compact"
              type="text"
              value={modelDraft}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              onChange={(e) => setModelDraft(e.target.value)}
              onBlur={commitModel}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
              className="w-48"
            />
          </SettingContainer>
        )}
      </SettingsGroup>
      <SettingsGroup title={t("settings.transform.style.title")}>
        <div role="radiogroup" aria-label={t("settings.transform.style.title")}>
          {STYLES.map((style) => {
            const checked = style === selected;
            return (
              <button
                key={style}
                type="button"
                role="radio"
                aria-checked={checked}
                disabled={isUpdating("transform_style")}
                onClick={() => updateSetting("transform_style", style)}
                className="flex w-full cursor-pointer items-center justify-between gap-4 px-4 p-2 text-start hover:bg-wash disabled:cursor-default"
              >
                <span>
                  <span className="block text-sm font-medium">
                    {t(`settings.transform.style.options.${style}.name`)}
                  </span>
                  <span className="block text-sm text-ink-3">
                    {t(`settings.transform.style.options.${style}.description`)}
                  </span>
                </span>
                <Radio checked={checked} />
              </button>
            );
          })}
        </div>
        <SettingContainer
          title={t("settings.transform.voiceGuide.title")}
          description={t("settings.transform.voiceGuide.description")}
          descriptionMode="inline"
          grouped={true}
        >
          <div className="flex items-center gap-2">
            {syncMessage && (
              <span className="text-sm text-ink-3" role="status">
                {syncMessage}
              </span>
            )}
            {syncConfigured && (
              <Button
                type="button"
                variant="secondary"
                size="sm"
                disabled={syncing}
                onClick={syncGuide}
              >
                {t("settings.transform.voiceGuide.sync")}
              </Button>
            )}
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => commands.openVoiceGuide()}
            >
              {t("settings.transform.voiceGuide.button")}
            </Button>
          </div>
        </SettingContainer>
        <SettingContainer
          title={guideName}
          description={t("settings.transform.voiceGuide.fileDescription")}
          descriptionMode="inline"
          grouped={true}
        >
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={chooseGuide}
            >
              {t("settings.transform.voiceGuide.choose")}
            </Button>
            {guidePath && (
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => updateSetting("voice_guide_path", null)}
              >
                {t("settings.transform.voiceGuide.useDefault")}
              </Button>
            )}
          </div>
        </SettingContainer>
      </SettingsGroup>
    </div>
  );
};
