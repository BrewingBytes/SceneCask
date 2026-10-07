import "../../../styles";

export interface RadioGroupProps {
  label: string;
  name: string;
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  options: readonly { value: string; label: string; disabled?: boolean }[];
  disabled?: boolean;
}
export function RadioGroup({
  label,
  name,
  value,
  defaultValue,
  onChange,
  options,
  disabled,
}: RadioGroupProps) {
  return (
    <fieldset className="sc-radio" disabled={disabled}>
      <legend>{label}</legend>
      {options.map((option) => (
        <label key={option.value} className="sc-choice">
          <input
            type="radio"
            name={name}
            value={option.value}
            checked={value === undefined ? undefined : value === option.value}
            defaultChecked={
              value === undefined ? defaultValue === option.value : undefined
            }
            readOnly={value !== undefined && !onChange}
            onChange={onChange ? () => onChange(option.value) : undefined}
            disabled={option.disabled}
          />
          <span>{option.label}</span>
        </label>
      ))}
    </fieldset>
  );
}
