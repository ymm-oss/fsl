// Typestate skeleton for `Order` from spec `OrderWorkflow`.
// FSL holds these in a collection; phantom types track one entity, so each becomes an independently-typed handle.
// Only transitions with a LOCAL from-state guard are typed; the rest stay dynamic.

export type OrderState = "Draft" | "Placed" | "Paid" | "Shipped" | "Cancelled";

declare const __state: unique symbol;
export interface Order<S extends OrderState> {
  qty: number;
  readonly [__state]: S;
}

  // runtime precondition (not in type): (q > 0)
export function place(self: Order<"Draft">, o: number, q: number): Order<"Placed">;
export function pay(self: Order<"Placed">, o: number): Order<"Paid">;
export function ship(self: Order<"Paid">, o: number): Order<"Shipped">;
export function cancel(self: Order<"Paid" | "Placed">, o: number): Order<"Cancelled">;
