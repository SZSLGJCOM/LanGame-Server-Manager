import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useMemo } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import type { AiSettings, PersistAiSettings } from "../ai-settings";
import type { AssistantConversationSummary } from "../assistant-conversations";
import type {
  AssistantActionId,
  AssistantBuildInput,
  AssistantCapsuleModel,
  AssistantPromptCard
} from "../assistant-types";
import { isChineseLocale, useI18n } from "../i18n";
import type {
  AssistantChatMessage,
  AssistantExecutionState,
  ThemeMode,
  ViewKey
} from "../types";
import { openExternalUrl } from "../api";
import langameDarkLogoUrl from "../assets/langame-logo-dark.svg";
import langameLogoUrl from "../assets/langame-logo.svg";
import { AssistantCapsule } from "./AssistantCapsule";
import { ShellIcon } from "./ShellIcon";
import { useWindowMaximized } from "../hooks/useWindowMaximized";
import { syncDesktopTrayLocale } from "../desktop-tray-locale";

interface HeaderNavItem {
  key: ViewKey;
  icon: "dashboard" | "server" | "package";
}

const NAV_ITEMS: HeaderNavItem[] = [
  { key: "system", icon: "dashboard" },
  { key: "library", icon: "package" },
  { key: "servers", icon: "server" }
];

const LANGAME_WEBSITE_URL = "https://langame.cn/";

interface AppHeaderProps {
  activeView: ViewKey;
  aiSettings: AiSettings;
  assistant: AssistantCapsuleModel;
  assistantDraft: string;
  assistantInput: AssistantBuildInput;
  assistantExecution: AssistantExecutionState;
  assistantConfirmationOpen?: boolean;
  assistantMessages: AssistantChatMessage[];
  assistantConversations: AssistantConversationSummary[];
  assistantActiveConversationId: string | null;
  serverCount: number | null;
  theme: ThemeMode;
  onAssistantAction: (actionId: AssistantActionId) => void | Promise<void>;
  onAssistantDeleteConversation: (conversationId: string) => void;
  onAssistantDraftChange: (value: string) => void;
  onAssistantNewConversation: () => void;
  onAssistantSelectConversation: (conversationId: string) => void;
  onAssistantRunPrompt: (prompt: AssistantPromptCard) => void | Promise<void>;
  assistantRecoveryPending?: boolean;
  assistantContinuation?: import("../types").AssistantContinuation | null;
  onAssistantResume?: () => void | Promise<void>;
  onAssistantStop?: () => void | Promise<void>;
  onAssistantSendMessage: (message: string) => void | Promise<void>;
  onClearAiSecret: PersistAiSettings;
  onSaveAiSettings: PersistAiSettings;
  onSelectView: (view: ViewKey) => void;
  onThemeChange: (theme: ThemeMode) => void;
}

export function AppHeader(props: AppHeaderProps) {
  const { locale, setLocale, t } = useI18n();
  const appWindow = useMemo(() => {
    if (!isTauri()) {
      return null;
    }

    try {
      return getCurrentWindow();
    } catch (error) {
      console.warn("Window API is not available in this runtime.", error);
      return null;
    }
  }, []);
  useEffect(() => {
    if (appWindow) {
      syncDesktopTrayLocale(locale);
    }
  }, [appWindow, locale]);
  const maximized = useWindowMaximized(appWindow);
  const nextTheme = props.theme === "dark" ? "light" : "dark";
  const themeIcon = props.theme === "dark" ? "moon" : "sun";
  const logoUrl = props.theme === "dark" ? langameDarkLogoUrl : langameLogoUrl;
  const localeLabel = t("shell.languageLabel");
  const chineseLocaleActive = isChineseLocale(locale);

  async function runWindowCommand(action: (window: NonNullable<ReturnType<typeof getCurrentWindow>>) => Promise<void>) {
    if (!appWindow) {
      return;
    }

    try {
      await action(appWindow);
    } catch (error) {
      console.error("Window control action failed", error);
    }
  }

  function isInteractiveWindowTarget(target: EventTarget | null) {
    if (!(target instanceof Element)) {
      return false;
    }

    return Boolean(
      target.closest("button, input, select, textarea, option, a, label, [role='button'], [data-no-window-drag='true']")
    );
  }

  function handleWindowDragMouseDown(event: ReactMouseEvent<HTMLElement>) {
    if (event.button !== 0 || isInteractiveWindowTarget(event.target)) {
      return;
    }
    event.preventDefault();
    void runWindowCommand((window) => window.startDragging());
  }

  function handleWindowDragDoubleClick(event: ReactMouseEvent<HTMLElement>) {
    if (isInteractiveWindowTarget(event.target)) {
      return;
    }
    event.preventDefault();
    void runWindowCommand((window) => window.toggleMaximize());
  }

  function minimizeWindow() {
    void runWindowCommand((window) => window.minimize());
  }

  function toggleWindowSize() {
    void runWindowCommand((window) => window.toggleMaximize());
  }

  function hideToTray() {
    void runWindowCommand((window) => window.close());
  }

  function renderAssistantCapsule() {
    return (
      <AssistantCapsule
        aiSettings={props.aiSettings}
        assistant={props.assistant}
        assistantDraft={props.assistantDraft}
        assistantInput={props.assistantInput}
        execution={props.assistantExecution}
        confirmationOpen={props.assistantConfirmationOpen}
        messages={props.assistantMessages}
        conversations={props.assistantConversations}
        activeConversationId={props.assistantActiveConversationId}
        onAction={props.onAssistantAction}
        onClearAiSecret={props.onClearAiSecret}
        onDeleteConversation={props.onAssistantDeleteConversation}
        onDraftChange={props.onAssistantDraftChange}
        onNewConversation={props.onAssistantNewConversation}
        onSaveAiSettings={props.onSaveAiSettings}
        onSelectConversation={props.onAssistantSelectConversation}
        onRunPrompt={props.onAssistantRunPrompt}
        recoveryPending={props.assistantRecoveryPending}
        continuation={props.assistantContinuation}
        onResume={props.onAssistantResume}
        onStop={props.onAssistantStop}
        onSendMessage={props.onAssistantSendMessage}
      />
    );
  }

  return (
    <header className="shell-header" onMouseDown={handleWindowDragMouseDown} onDoubleClick={handleWindowDragDoubleClick}>
      <div className="shell-header-row">
        <div className="shell-header-side shell-header-side--left">
          <div className="shell-window-brand">
            <button
              type="button"
              className="shell-window-brand-link"
              data-no-window-drag="true"
              aria-label={t("shell.openOfficialWebsite")}
              title={t("shell.openOfficialWebsite")}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={() => void openExternalUrl(LANGAME_WEBSITE_URL)}
            >
              <img
                className="shell-window-brand-logo"
                src={logoUrl}
                alt=""
                width={1734}
                height={261}
              />
            </button>
          </div>
        </div>

        <div className="shell-header-center">
          {renderAssistantCapsule()}
        </div>

        <div className="shell-header-side shell-header-side--right">
          <div className="shell-header-tools">
            <button
              type="button"
              className="shell-theme-icon-button"
              aria-label={t(`shell.${nextTheme === "dark" ? "themeDark" : "themeLight"}`)}
              title={t(`shell.${nextTheme === "dark" ? "themeDark" : "themeLight"}`)}
              onClick={() => props.onThemeChange(nextTheme)}
            >
              <ShellIcon name={themeIcon} className="shell-small-icon" />
            </button>

            <button
              type="button"
              className="shell-locale-button"
              aria-label={localeLabel}
              title={localeLabel}
              onClick={() => setLocale(chineseLocaleActive ? "en-US" : "zh-CN")}
            >
              {chineseLocaleActive ? "EN" : "ZH"}
            </button>

            <div className="shell-window-controls">
              <button
                type="button"
                className="shell-window-button"
                onClick={minimizeWindow}
                aria-label={t("common.minimize")}
                title={t("common.minimize")}
              >
                <ShellIcon name="minus" className="shell-small-icon" />
              </button>
              <button
                type="button"
                className="shell-window-button"
                onClick={toggleWindowSize}
                aria-label={t(maximized ? "common.restoreWindow" : "common.maximize")}
                title={t(maximized ? "common.restoreWindow" : "common.maximize")}
              >
                <ShellIcon name={maximized ? "restore" : "square"} className="shell-small-icon" />
              </button>
              <button
                type="button"
                className="shell-window-button is-danger"
                onClick={hideToTray}
                aria-label={t("common.minimizeToTray")}
                title={t("common.minimizeToTray")}
              >
                <ShellIcon name="x" className="shell-small-icon" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <div className="shell-header-nav-row">
        <nav className="shell-topnav" aria-label={t("shell.primaryNavigation")}>
          {NAV_ITEMS.map((item) => {
            const active = item.key === props.activeView;
            const showBadge = item.key === "servers" && props.serverCount !== null;
            return (
              <button
                key={item.key}
                type="button"
                className={active ? "shell-topnav-item is-active" : "shell-topnav-item"}
                aria-current={active ? "page" : undefined}
                onClick={() => props.onSelectView(item.key)}
              >
                <ShellIcon name={item.icon} className="shell-topnav-icon" aria-hidden="true" />
                <span className="shell-topnav-label">{t(`nav.${item.key}.label`)}</span>
                {showBadge ? <span className="shell-topnav-badge">{props.serverCount}</span> : null}
              </button>
            );
          })}
        </nav>
      </div>
    </header>
  );
}
