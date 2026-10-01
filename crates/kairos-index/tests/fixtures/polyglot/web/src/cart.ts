import { formatPrice, Money } from "./format";

export class Cart {
  private items: Money[] = [];

  add(item: Money): void {
    this.items.push(item);
  }

  total(): string {
    const cents = this.items.reduce((sum, item) => sum + item.cents, 0);
    return formatPrice({ cents, currency: "EUR" });
  }
}
