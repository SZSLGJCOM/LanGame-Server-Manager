import { useMemo } from "react";
import { buildBindAddressOptions, formatBindAddressCandidateLabel } from "../../bind-addresses";
import { useI18n } from "../../i18n";
import type { BindAddressCandidate } from "../../types";

interface ListenAddressSelectProps {
  value: string;
  candidates: BindAddressCandidate[];
  disabled?: boolean;
  onChange: (value: string) => void;
}

export function ListenAddressSelect(props: ListenAddressSelectProps) {
  const { locale } = useI18n();
  const options = useMemo(
    () => buildBindAddressOptions(props.candidates, props.value),
    [props.candidates, props.value]
  );

  return (
    <select
      className="settings-schema-input settings-schema-select"
      value={props.value}
      disabled={props.disabled}
      onChange={(event) => props.onChange(event.target.value)}
    >
      {options.map((candidate) => (
        <option key={`${candidate.kind}-${candidate.address}`} value={candidate.address}>
          {formatBindAddressCandidateLabel(candidate, locale)}
        </option>
      ))}
    </select>
  );
}
