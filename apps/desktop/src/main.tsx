import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { I18nProvider } from "./i18n";
import { StartupErrorBoundary } from "./components/StartupErrorBoundary";
import { InstanceSettingsSaveProvider } from "./views/settings/InstanceSettingsSaveContext";
import "./app.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <StartupErrorBoundary>
      <I18nProvider>
        <InstanceSettingsSaveProvider>
          <App />
        </InstanceSettingsSaveProvider>
      </I18nProvider>
    </StartupErrorBoundary>
  </React.StrictMode>
);
