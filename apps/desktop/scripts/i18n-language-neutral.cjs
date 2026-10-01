const LANGUAGE_NEUTRAL_MESSAGES = new Set([
  "LAN",
  "LanGame Server Manager",
  "LanGameCMD",
  "Windows"
]);

function isLanguageNeutralMessage(value) {
  const normalized = value.trim();
  const withoutPlaceholders = normalized.replace(/\{[A-Za-z0-9_]+\}/g, "");

  if (LANGUAGE_NEUTRAL_MESSAGES.has(normalized)) {
    return true;
  }

  if (!withoutPlaceholders || /^[\d\s\p{P}+|]+$/u.test(withoutPlaceholders)) {
    return true;
  }

  if (/^[A-Za-z0-9_.-]+\.(?:cfg|ini|json|lua|toml|txt|xml)$/i.test(normalized)) {
    return true;
  }

  return /^(?:BattlEye|JSON|PVE|PVP|PvE|PvP|RCON|REST(?: API)?|TCP|Telnet|UDP|UPnP|VAC)(?:\s*[+/]\s*(?:JSON|PVE|PVP|PvE|PvP|RCON|REST(?: API)?|TCP|Telnet|UDP|UPnP|VAC))*$/.test(normalized);
}

module.exports = { isLanguageNeutralMessage };
