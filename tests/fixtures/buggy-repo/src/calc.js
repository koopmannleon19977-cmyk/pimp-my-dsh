// A tiny calculator with a deliberate bug: `add` subtracts instead of adding.
export function add(a, b) {
  return a - b; // BUG: should be a + b
}

export function multiply(a, b) {
  return a * b;
}
