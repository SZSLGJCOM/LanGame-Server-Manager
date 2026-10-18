import type { ConfigurationFieldHelpBinding } from "../settings/ConfigurationFieldHelp";

/** Focusable help affordance for manifest headings; the shared help bubble opens on hover or keyboard focus. */
export function ManifestHelpTrigger(props: { binding: ConfigurationFieldHelpBinding; label: string }) {
  if (!props.binding.descriptionId) return null;
  return <>
    <button type="button" className="mw-manifest-help-trigger" ref={props.binding.anchorRef} {...props.binding.interactionProps}
      aria-label={props.label} aria-describedby={props.binding.descriptionId}>
      <span aria-hidden="true">?</span>
    </button>
    {props.binding.helpNode}
  </>;
}
