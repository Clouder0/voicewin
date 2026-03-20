export function summarizeNumbers(values: number[]): { min: number; avg: number; max: number } | null {
  if (values.length === 0) return null;
  return {
    min: Math.min(...values),
    avg: Math.trunc(values.reduce((sum, value) => sum + value, 0) / values.length),
    max: Math.max(...values),
  };
}

export function formatBenchmarkLatency(
  minMs: number | null,
  avgMs: number | null,
  maxMs: number | null,
  label: string,
): string | null {
  if (minMs == null || avgMs == null || maxMs == null) return null;
  return `${label} ${minMs}/${avgMs}/${maxMs} ms min/avg/max`;
}

export function formatBenchmarkCountRange(
  minValue: number | null,
  avgValue: number | null,
  maxValue: number | null,
  label: string,
  unit: string,
): string | null {
  if (minValue == null || avgValue == null || maxValue == null) return null;
  return `${label} ${minValue}/${avgValue}/${maxValue} ${unit} min/avg/max`;
}
