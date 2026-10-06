const origin = process.env.SMOKE_ORIGIN ?? "http://127.0.0.1:3000";
for (const [path, status] of [["/health/live", "live"], ["/health/ready", "ready"]]) {
  const response = await fetch(`${origin}${path}`, { signal: AbortSignal.timeout(5000) });
  const body = await response.json();
  if (response.status !== 200 || body.status !== status) throw new Error(`Health smoke failed: ${path} (${response.status})`);
  console.log(`${path}: 200 ${status}`);
}
