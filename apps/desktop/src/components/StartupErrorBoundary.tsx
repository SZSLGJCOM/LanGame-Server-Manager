import { Component, type ErrorInfo, type ReactNode } from "react";
import { isChineseLocale } from "../i18n-config";

interface StartupErrorBoundaryProps {
  children: ReactNode;
}

interface StartupErrorBoundaryState {
  hasError: boolean;
  message: string;
}

export class StartupErrorBoundary extends Component<StartupErrorBoundaryProps, StartupErrorBoundaryState> {
  state: StartupErrorBoundaryState = {
    hasError: false,
    message: ""
  };

  static getDerivedStateFromError(error: Error) {
    return {
      hasError: true,
      message: error.message || ""
    };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("Startup render error:", error, errorInfo);
  }

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    const language = typeof document === "undefined" ? "en-US" : document.documentElement.lang;
    const copy = isChineseLocale(language)
      ? {
          title: "启动初始化失败",
          description: "界面在完成渲染前遇到运行错误。请记录下方信息并重新加载应用。",
          unknownError: "未知启动错误",
          reload: "重新加载"
        }
      : {
          title: "Startup initialization failed",
          description: "The interface encountered a runtime error before rendering. Record the details below, then reload the app.",
          unknownError: "Unknown startup error",
          reload: "Reload app"
        };

    return (
      <div
        aria-live="assertive"
        role="alert"
        style={{
          minHeight: "100vh",
          display: "grid",
          placeItems: "center",
          padding: "20px",
          background: "#0a0f16",
          color: "#f3f5f8",
          fontSize: "14px"
        }}
      >
        <div style={{ maxWidth: "720px", width: "100%", display: "grid", gap: "12px" }}>
          <h2 style={{ margin: 0, fontSize: "18px" }}>{copy.title}</h2>
          <p style={{ margin: 0 }}>{copy.description}</p>
          <pre
            style={{
              margin: 0,
              padding: "12px",
              borderRadius: "10px",
              border: "1px solid rgba(255, 255, 255, 0.16)",
              background: "#172433",
              color: "#ffb4b4",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              maxHeight: "220px",
              overflow: "auto"
            }}
          >
            {this.state.message || copy.unknownError}
          </pre>
          <button
            type="button"
            onClick={() => window.location.reload()}
            style={{
              justifySelf: "start",
              padding: "8px 12px",
              borderRadius: "8px",
              border: "1px solid rgba(255, 255, 255, 0.16)",
              background: "#21314b",
              color: "#f3f5f8",
              cursor: "pointer"
            }}
          >
            {copy.reload}
          </button>
        </div>
      </div>
    );
  }
}
