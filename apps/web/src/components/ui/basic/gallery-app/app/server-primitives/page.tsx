import { Field } from "../../../field";
import { Avatar } from "../../../avatar";
import { Badge } from "../../../badge";
import { Checkbox } from "../../../checkbox";
import { EmptyState } from "../../../empty-state";
import { Progress } from "../../../progress";
import { RadioGroup } from "../../../radio-group";
import { Skeleton } from "../../../skeleton";
export default function ServerPrimitives() {
  return (
    <main className="sc-foundation">
      <Field label="Server field" hint="Server hint">
        {(props) => <input {...props} />}
      </Field>
      <Badge>Server badge</Badge>
      <Avatar initials="🇷🇴🇺🇸" label="Server avatar" />
      <Progress value={25} label="Server progress" />
      <Skeleton label="Server loading" />
      <EmptyState title="Server empty" />
      <Checkbox label="Native checkbox" />
      <RadioGroup
        label="Native choices"
        name="choices"
        defaultValue="first"
        options={[
          { value: "first", label: "First native choice" },
          { value: "second", label: "Second native choice" },
        ]}
      />
    </main>
  );
}
