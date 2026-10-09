// A JavaScript file of the fixture (KAIROS-T-0352): a function, a class
// with a method, and a call to a TypeScript function of the same family.
import { formatPrice } from "./format";

export function roundCents(value) {
  return Math.round(value * 100) / 100;
}

export class Ledger {
  entry(value) {
    return formatPrice(roundCents(value));
  }
}
