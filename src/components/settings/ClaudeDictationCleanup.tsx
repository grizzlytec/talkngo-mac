import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";

interface ClaudeDictationCleanupProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ClaudeDictationCleanup: React.FC<ClaudeDictationCleanupProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const enabled = getSetting("dictation_claude_cleanup") ?? false;
    const [hasKey, setHasKey] = useState(false);

    useEffect(() => {
      commands
        .claudeKeyStatus()
        .then((result) => {
          if (result.status === "ok") setHasKey(result.data.saved);
        })
        .catch(() => undefined);
    }, []);

    return (
      <div>
        <ToggleSwitch
          checked={enabled && hasKey}
          onChange={(next) => updateSetting("dictation_claude_cleanup", next)}
          isUpdating={isUpdating("dictation_claude_cleanup")}
          disabled={!hasKey}
          label={t("settings.advanced.claudeDictationCleanup.title")}
          description={t(
            "settings.advanced.claudeDictationCleanup.description",
          )}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        {hasKey ? (
          <p className="px-4 pb-3 text-xs opacity-60">
            {t("settings.advanced.claudeDictationCleanup.shortcutNote")}
          </p>
        ) : (
          <p className="px-4 pb-2 text-sm text-red-600">
            {t("settings.advanced.claudeDictationCleanup.noKey")}
          </p>
        )}
      </div>
    );
  });
