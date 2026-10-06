/** Resolve local JSON pointers without mutating the contract. */
export function resolveLocal(spec, value) {
  if (!value?.$ref) return value;
  if (!value.$ref.startsWith("#/")) throw new Error("Contract references must be local.");
  return value.$ref.slice(2).split("/").reduce((node, key) =>
    node[key.replace(/~1/g, "/").replace(/~0/g, "~")], spec);
}

/** Operation parameters override inherited path parameters by (in, name), per OpenAPI. */
export function listContractOperations(spec) {
  const methods = new Set(["get", "post", "put", "patch", "delete", "head", "options", "trace"]);
  return Object.entries(spec.paths).flatMap(([route, source]) => {
    const item = resolveLocal(spec, source);
    return Object.entries(item).filter(([method]) => methods.has(method)).map(([method, operation]) => {
      const parameters = new Map();
      for (const parameter of [...(item.parameters ?? []), ...(operation.parameters ?? [])]) {
        const resolved = resolveLocal(spec, parameter);
        parameters.set(`${resolved.in}:${resolved.name}`, resolved);
      }
      return { route, method, operation, parameters: [...parameters.values()] };
    });
  });
}
