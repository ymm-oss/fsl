Fixed (#779): a saga step, timeout, or compensation action that emits an
event no longer leaves that event's declared `evolve` unapplied. Both native
`domain` lowering paths (`lower_saga_actions` in `domain_lowering.rs`,
`render_saga_actions` in `domain.rs`) called `event_assignments` for these
three action kinds without the paired `evolve_items`/`saga_emit_evolve` call,
so an action could raise its emitted event's one-hot flag while the aggregate
state that event was declared to evolve stayed frozen at its initial value
forever — an accepted-but-unreachable-transition soundness defect in the same
class PR #725 (#713) closed for the compensation guard. Both paths now apply
the declared evolve in the same action, in emit order, matching the pairing
already correct for command/decide, effect-completion, and saga-observe
actions. A new corpus-wide sweep
(`rust/fsl-core/tests/domain_saga_evolve_pairing.rs`) asserts, for every
action any domain fixture lowers to on either path, that an action setting an
event flag true also applies that event's declared evolve, so a future
lowering rewrite (e.g. #679's saga-history rewrite) cannot silently drop the
pairing again. Negative controls on `examples/domain/order_fulfillment_saga.fsl`
confirm both the fix (`inventory_status != ReservationPending` and
`payment_status != PaymentPending`, previously `proved` under
`--engine induction`, now correctly `violated`) and its boundary
(`inventory_status != ReleaseRequested` stays `proved`, and the saga's
never-enabled compensation warning is unchanged, because the compensation
action's dual event-flag guard is untouched by this fix and remains
structurally disabled until #679).
