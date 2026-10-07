export const classes = (...names: (string | undefined | false)[]) =>
  names.filter(Boolean).join(" ");
