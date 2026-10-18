import { useEffect, useState } from "react";
import privacyDocument from "../../../../PRIVACY.md?raw";
import { fetchAppVersion } from "../api";
import { isChineseLocale, useI18n } from "../i18n";
import { readPrivacyNoticeSections } from "../privacy-notice-document";
import "./privacy-disclosure.css";

const notices = {
  chinese: readPrivacyNoticeSections(privacyDocument, "简体中文"),
  english: readPrivacyNoticeSections(privacyDocument, "English")
};

export function PrivacyNotice({ className = "" }: { className?: string }) {
  const { locale } = useI18n();
  const chinese = isChineseLocale(locale);
  const sections = chinese ? notices.chinese : notices.english;
  const [version, setVersion] = useState<string | null>(null);
  const [versionLoading, setVersionLoading] = useState(true);
  useEffect(() => {
    let active = true;
    void fetchAppVersion().then((currentVersion) => {
      if (active) {
        setVersion(currentVersion);
        setVersionLoading(false);
      }
    }, () => {
      if (active) setVersionLoading(false);
    });
    return () => { active = false; };
  }, []);
  return <div className={`privacy-notice-body ${className}`} tabIndex={0} role="region"
      aria-label={chinese ? "完整隐私说明（离线可读）" : "Full privacy notice (available offline)"}>
      {sections.map((section) => <section key={section.title}>
        <h3>{section.title}</h3>
        {section.paragraphs.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}
      </section>)}
      <footer className="privacy-notice-app-info" aria-label={chinese ? "应用信息" : "Application information"}>
        <span>LanGame Server Manager</span>
        <span className="privacy-notice-version" role="status" aria-busy={versionLoading}>
          {versionLoading ? (chinese ? "正在读取版本…" : "Loading version…")
            : version ? <>{chinese ? "当前版本" : "Current version"} <bdi>v{version}</bdi></>
            : (chinese ? "版本信息暂不可用" : "Version unavailable")}
        </span>
      </footer>
    </div>;
}
