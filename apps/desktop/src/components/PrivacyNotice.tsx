import privacyDocument from "../../../../PRIVACY.md?raw";
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
  return <div className={`privacy-notice-body ${className}`} tabIndex={0} role="region"
      aria-label={chinese ? "完整隐私说明（离线可读）" : "Full privacy notice (available offline)"}>
      {sections.map((section) => <section key={section.title}>
        <h3>{section.title}</h3>
        {section.paragraphs.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}
      </section>)}
    </div>;
}
