export interface PrivacyNoticeSection {
  title: string;
  paragraphs: string[];
}

/** PRIVACY.md deliberately uses only language headings, section headings and paragraphs. */
export function readPrivacyNoticeSections(document: string, language: "English" | "简体中文"): PrivacyNoticeSection[] {
  const sections: PrivacyNoticeSection[] = [];
  let selected = false;
  for (const block of document.trim().split(/\r?\n\s*\r?\n/)) {
    const text = block.trim();
    if (text.startsWith("## ")) {
      selected = text === `## ${language}`;
    } else if (selected && text.startsWith("### ")) {
      sections.push({ title: text.slice(4), paragraphs: [] });
    } else if (selected && sections.length > 0) {
      sections[sections.length - 1].paragraphs.push(text);
    }
  }
  return sections;
}
