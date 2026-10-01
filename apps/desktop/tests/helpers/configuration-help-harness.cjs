const React = require("react");

// Interaction/lifecycle tests replace the help boundary; real tooltip behavior
// is exercised by the browser fixture with the production hook and portal.
function useConfigurationFieldHelp(id, description) {
  return { anchorRef() {}, interactionProps: {}, descriptionId: description ? id : undefined,
    helpNode: description ? React.createElement("span", { id, className: "configuration-field-help-description" }, description) : null };
}
function ConfigurationHelp({ description, children }) {
  const help = useConfigurationFieldHelp(`help-${String(description).replace(/[^a-z0-9]/gi, "-")}`, description);
  return React.createElement(React.Fragment, null, children(help), help.helpNode);
}
module.exports = { useConfigurationFieldHelp, ConfigurationHelp };
