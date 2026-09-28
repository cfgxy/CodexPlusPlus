export function maskSecretValue(value: string, emptyLabel: string): string {
  const trimmed = value.trim();
  if (!trimmed) return emptyLabel;
  if (trimmed.length <= 8) return "••••";
  if (trimmed.length <= 10) return `${trimmed.slice(0, 2)}…${trimmed.slice(-2)}`;
  return `${trimmed.slice(0, 6)}…${trimmed.slice(-4)}`;
}
