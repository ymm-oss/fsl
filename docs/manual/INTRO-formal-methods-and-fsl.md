# Introduction to Formal Methods and FSL

## Purpose

This document organizes the basic ideas of formal methods, the role of FSL (AI-Native Formal Specification Language), how to use it from the business level through implementation and QA, and the points needed to decide on adoption — all as a foundation for introducing FSL into AI-driven development.

The intended readers are business owners, PMs/PdMs, QA, engineers, and designers of AI-driven development processes. No prior expertise in formal methods or model checking is assumed.

## Why FSL is needed

Generative AI speeds up organizing specifications, implementation, writing tests, and refactoring. At the same time, it increases risks such as AI plausibly filling in specifications, dropping exceptional cases, and generating implementation and tests from the same misunderstanding.

Conventional natural-language specification documents and ordinary tests alone cannot continuously keep up with checking the diffs that AI produces at high speed. In particular, when business rules, requirements, design, implementation, and QA are managed as separate artifacts, it becomes hard to confirm whether the upper-level intent is correctly reflected in the lower-level implementation and tests.

FSL is used to address this problem by describing business rules and requirements as contracts that a machine can check. The goal is to check AI-generated specifications, implementations, and tests against those contracts, and to find contradictions, loopholes, unreachable flows, and implementation deviations at an early stage.

Another important point is that FSL is not premised on humans spending a long time writing large, heavyweight formal specifications; it is premised on generative AI quickly producing small specifications and fixing them while looking at the verifier's counterexamples. As a result, it applies easily not only to high-risk areas such as payments and approvals, but also to everyday state management such as screen transitions, wizards, modals, double-submission prevention, and small flag management.

## What is the benefit of having FSL

With FSL, you can treat specifications not only as something to "read" but as something to "check." This yields the following effects at each stage of AI-driven development.

- Find ambiguities, contradictions, and missing exceptional cases in specifications before implementation
- Confirm, as a counterexample trace, the operation sequence that reaches a state that must never occur
- Confirm combinations of small screen transitions and state flags as a state machine before implementation
- Check whether necessary business goals and acceptance criteria can be reached
- Turn acceptance criteria into executable scenarios with `fslc scenarios`
- Generate pytest conformance-test scaffolds from a specification with `fslc testgen`
- Replay an implementation's event log with `fslc replay` and check for specification violations
- Connect business, requirements, design, and implementation tests vertically through refinement
- Confirm AI-produced code and tests by the verifier's results rather than by the AI's own explanation

In short, FSL is not a tool that directly speeds up code generation; it is an external standard for continuously checking the artifacts that AI produces at high speed.

## Summary

Ordinary specification documents are written in natural language, which makes them easy for humans to read but prone to leaving ambiguity, differences in interpretation, contradictions, and missing exceptional cases. Formal methods are an approach for describing specifications in a form a machine can check, and for verifying questions such as "does it reach a state that must never occur?", "can it reach the states it needs to?", and "can the rules be broken by some order of operations?".

FSL is a formal specification language for connecting these formal methods to application development, especially to development processes that use generative AI. It puts business rules, requirements, design specifications, and implementation-conformance tests on the same verification loop, and serves as a harness for mechanically checking AI-generated specifications, code, and tests.

The value of adopting FSL is not in directly speeding up code generation, but in providing a continuously checkable external standard for the artifacts that generative AI produces at high speed. Moreover, because generative AI lowers the initial cost of authoring specifications, FSL can be used as a lightweight check even for small state management that conventional formal methods would not have been worth the effort.

## What are formal methods

Formal methods is the collective term for techniques that describe a system's specification or design in a form that can be handled mathematically and mechanically, and that check or prove its properties.

The properties one typically wants to confirm in software development are like the following.

- An unapproved order cannot be shipped
- A cancelled application does not proceed to payment processing
- The reserved quantity and committed quantity of inventory do not contradict each other
- A user without permission cannot change the state
- A job placed in the queue is eventually processed
- A process that exceeds its SLA is not left unattended

These can be written in natural language as well. But in natural-language form, you cannot mechanically confirm whether the specification is truly consistent, whether it cannot be broken under any order of operations, or whether there are loopholes in the exceptional cases.

Formal methods handle a system by separating it into "states," "operations," and "properties that must hold."

```text
States:
  An order is one of Draft / Submitted / Approved / Cancelled / Shipped

Operations:
  submit / approve / cancel / ship

Invariants:
  An order that is not Approved does not become Shipped
  A Cancelled order does not become Shipped
```

The verifier explores the combinations of states and operations within the defined range and checks whether there is any operation sequence that breaks the conditions. If there is a violation, it returns "in what order of operations it breaks" as the shortest counterexample trace.

## Difference from testing

A test confirms a concrete example chosen by a developer or QA.

```text
1. Create an order
2. Approve it
3. Ship it
4. Confirm that it becomes Shipped
```

This is important, but what it confirms is a single chosen path. Formal methods explore the orders of operations and combinations of states within the range defined as the specification.

```text
No matter what order submit / approve / cancel / ship are called in,
an order that is not Approved never becomes Shipped.
```

Therefore, formal methods are not a replacement for testing. Their roles differ.

| Aspect | Ordinary testing | Formal methods |
|---|---|---|
| Target of confirmation | Chosen concrete examples | The defined state space |
| Main strength | Can confirm implementation, UI, and external integrations | Finds specification contradictions and loopholes early |
| Main weakness | Unselected cases are missed | Cannot check anything outside the modeled range |
| Artifacts | Test code, test cases | Formal specifications, counterexamples, proofs, generated scenarios |

In practice, the realistic approach is to confirm the safety of specifications and designs with formal methods, and to connect the scenarios and test scaffolds generated from those results to ordinary testing.

## Why formal methods become necessary in AI-driven development

Generative AI accelerates implementation, testing, documentation, and refactoring. At the same time, the following risks increase.

- AI plausibly fills in specifications
- Implementation and tests are generated from the same misunderstanding
- Exceptional cases and boundary conditions are missed
- The diff to review grows large, and humans cannot keep up
- It becomes hard to confirm whether a natural-language specification change was correctly reflected in the lower-level design and implementation

For this reason, in AI-driven development you need to prepare an external checking standard rather than trusting the AI's output as is.

Formal specifications become that checking standard.

```text
A human defines the business intent
  ↓
AI writes a first draft of the formal specification
  ↓
The verifier returns contradictions, counterexamples, and unreachability
  ↓
AI reads the counterexamples and proposes fixes
  ↓
A human makes the business decision on whether to adopt them
```

Through this loop, AI can be used not merely as a code generator, but as a worker that repairs specifications and implementations in response to feedback from the verifier.

## Difference in usage from conventional formal methods

Conventional formal methods required the expertise and working time to model specifications accurately, so adoption tended to be biased toward high-safety systems and complex infrastructure design. In practice, there are many small features for which "we'd like to check it, but it isn't worth the cost of writing a formal specification."

FSL changes this cost structure by being premised on generative AI writing the first draft of a specification, `fslc` mechanically returning counterexamples, and AI reading those counterexamples to propose fixes. Rather than a human writing a complete formal specification from the start, the human supplies the business intent, the states to forbid, and the states to reach, and the specification is fleshed out through the loop between AI and the verifier.

For this reason, FSL can be used not only as "a large tool used only for important core business," but also as "a small tool for quickly checking any place where state is even slightly involved." For example, the following specifications can be checked with just a few states and a few actions.

- Show a confirmation modal on the edit screen only when there are unsaved changes
- Allow only saved applications to be submitted
- Prevent double submission while submitting
- Allow retry after an error, but not after success
- A user without permission cannot advance the state

At this granularity, the realistic use of FSL is not as a formal design document, but as an aid to reviewing state transitions, an external standard for AI-generated code, and a concretization of QA perspectives.

For example, the unsaved-changes confirmation on an edit screen can be treated as the following small state machine.

```fsl
spec EditScreenFlow {
  enum Screen { List, Detail, Edit, Confirm }

  state {
    screen: Screen,
    dirty: Bool,
    submitting: Bool
  }
  init {
    screen = List
    dirty = false
    submitting = false
  }

  action open_detail() {
    requires screen == List
    screen = Detail
  }

  action start_edit() {
    requires screen == Detail
    screen = Edit
    dirty = false
  }

  action change() {
    requires screen == Edit
    requires not submitting
    dirty = true
  }

  action request_back() {
    requires screen == Edit
    requires dirty
    requires not submitting
    screen = Confirm
  }

  action discard() {
    requires screen == Confirm
    screen = Detail
    dirty = false
  }

  action save_start() {
    requires screen == Edit
    requires dirty
    requires not submitting
    submitting = true
  }

  action save_done() {
    requires screen == Edit
    requires submitting
    submitting = false
    dirty = false
  }

  invariant ConfirmOnlyWhenDirty { screen == Confirm => dirty }
  invariant SubmitOnlyWhileEditing { submitting => screen == Edit }
  reachable CanShowConfirm { screen == Confirm }
}
```

Even a specification of this size can make state-transition gaps such as "transitioning to the confirmation modal when not dirty," "becoming submitting outside the edit screen," and "being unable to reach the back-confirmation" into targets for checking.

## What is FSL

FSL is a formal specification language for application development, designed on the premise that generative AI writes, verifies, and repairs it. The verifier `fslc` performs syntax and type checking, model checking, k-induction, scenario generation, implementation-conformance test generation, log replay, and refinement checking against a specification.

A distinguishing feature of FSL is that it does not confine formal specifications to the engineering design layer alone, but handles them vertically from business through requirements, design, and the implementation connection.

```text
Business FSL
  ↓ refinement
Requirements FSL
  ↓ refinement
Design Spec FSL
  ↓ scenarios / testgen / replay
Implementation / QA
```

The main commands are as follows.

```bash
fslc check file.fsl
fslc verify file.fsl --depth 8
fslc verify file.fsl --engine induction
fslc scenarios file.fsl
fslc testgen file.fsl -o tests/test_conformance.py
fslc replay file.fsl --trace events.json
fslc refine impl.fsl abs.fsl mapping.fsl
```

## The layers FSL handles

FSL lets you handle the same domain at multiple granularities.

### Business layer

Handles business rules, regulations, policies, KPIs, and business goals. The purpose is to confirm whether there are contradictory rules as a business matter, whether forbidden states are reached, and whether the necessary business goals can be reached.

Examples:

- Returns are limited to within 30 days after shipment
- Only approved expenses become eligible for payment
- A paid application cannot be sent back

### Requirements layer

Describes the requests, acceptance criteria, exceptional cases, and branches that PMs/PdMs handle. By retaining the requirement ID and the original text, you can trace from verification results and counterexamples back to the original request.

Examples:

- `REQ-1: An order can be committed only after inventory is secured`
- `AC-1: An order cannot be committed when there is no inventory`

### Design layer

Describes the state machines, data structures, actions, asynchronous processing, and external-integration states that are close to the implementation. Here you use invariant, reachable, leadsTo, refinement mappings, and so on to confirm whether the design layer satisfies the requirements layer.

### Implementation / QA layer

Generate executable scenarios from a specification with `fslc scenarios`, and generate pytest conformance-test scaffolds with `fslc testgen`. The implementation is connected through an Adapter.

```python
class Adapter:
    def reset(self):
        ...

    def step(self, action: str, params: dict):
        ...

    def observe(self) -> dict:
        ...
```

The Adapter's role is to map FSL actions to the implementation's APIs or service calls, and to observe the implementation's current state in the same form as the FSL state.

## Role in harness engineering

In harness engineering for AI-driven development, FSL is used as a verification harness that constrains generative AI.

```text
Natural-language specification
  ↓ AI turns it into FSL
FSL specification
  ↓ verify / scenarios / testgen
Verification harness
  ↓ AI generates the implementation and Adapter
Implementation
  ↓ pytest / replay / monitor
Conformance decision
```

Particularly important is that when test code is also generated by AI, there is a danger that the tests follow the implementation's misunderstanding. By placing FSL first, you can make both the implementation and the tests conform to an external specification.

## Anticipated use cases

FSL is not something to apply uniformly to every feature. There are broadly two entry points for adoption.

One is areas where errors in state transitions or business rules are serious. This is a usage close to conventional formal methods, focusing checking on places with high business risk.

Areas where the effect is high:

- Payments, refunds, billing
- Applications, approvals, send-backs
- Reservations, inventory, inventory allocation
- Permissions, authorization, audit logs
- Queues, jobs, asynchronous processing
- SLA, timeout, retry
- Contract states, plan changes, cancellation

The other is small state management that becomes worthwhile only because generative AI assists with authoring the specification. Here, rather than producing a heavyweight specification document, you use it for pre-implementation state-transition review and as a lightweight external standard for AI-generated code.

Areas that are small and easy to use:

- Screen transitions, back, cancel, confirmation modals
- UI states of editing, unsaved, saving, saved, error
- Wizards, step forms, onboarding
- Double-submission prevention, forbidding operations while loading
- Asynchronous states of retry / pending / succeeded / failed
- Feature flags, permissions, operation availability by role
- Small queues, notifications, badges, read/unread states

Areas with low adoption priority:

- Screens with only static display
- Simple CRUD
- UI decoration with few business constraints
- Processing with almost no state transitions

The criterion is not "how many states there are," but "whether some combination of operation order and flags can enter an unintended state." Even with 3 states and 2 flags, if back, cancel, retry, or permission branching is involved, it is worth turning into FSL.

## Proposed adoption process

FSL can be introduced not only at the start of development, but also into projects where the specification is already settled or development has partly begun. In that case, rather than turning everything into FSL at once, choose one high-risk business flow, or one screen/feature that is small but has many state branches.

When starting lightweight:

1. Enumerate the screens, states, flags, and operations
2. Write, in natural language, the "states that must never occur" and the "states that should be reachable"
3. Have the AI produce a first draft of a small `spec`
4. Run `fslc check` and `fslc verify`
5. Look at the counterexample traces and fix specification gaps, UI-behavior gaps, and errors in the implementation approach
6. If needed, pass representative scenarios from `fslc scenarios` to QA perspectives or E2E tests

When connecting all the way to implementation conformance:

1. Choose the target flow
2. Gather existing specification documents, tickets, QA perspectives, and implementation code
3. Have the AI produce a first draft of the FSL
4. Confirm syntax and types with `fslc check`
5. Confirm counterexamples and unreachability with `fslc verify`
6. Classify counterexamples into specification bugs, implementation bugs, undefined requirements, and modeling mistakes
7. Generate scenarios with `fslc scenarios`
8. Generate pytest scaffolds with `fslc testgen`
9. Connect the Adapter to the implementation
10. Incorporate verify and pytest into CI

With either adoption method, you can verify the effect of adopting FSL locally. In the lightweight usage, you need not rush to the Adapter or CI connection; it is fine to first just confirm "whether you can find state-transition errors with the verifier."

## Evaluation perspectives

When deciding whether to adopt FSL, confirm the following.

### Specification side

- Does the target domain have clear state transitions?
- Can you define forbidden states, invariants, and states you want to reach?
- Are exceptional cases and boundary conditions important for the business?
- Do any ambiguities remain in the existing specification documents?

### Implementation side

- Can the implementation be reset to its initial state?
- Can you call the APIs or functions that correspond to the FSL actions?
- Can the current state be obtained or projected via `observe()`?
- Can you obtain event logs in the staging or test environment?

### QA side

- Can the existing acceptance criteria be turned into scenarios?
- Can QA perspectives be expressed as state transitions or invariants?
- Do the generated scenarios complement the existing E2E tests?
- Can counterexample traces be handled in a reviewable form?

### Organization side

- Is it clear which business rules humans should make the final decision on?
- Is there someone to review the AI-generated FSL?
- Is it operationally feasible to stop on verification failure in CI?
- Is the scope of responsibility clear for also changing the FSL when the specification changes?

## Caveats

FSL is not omnipotent. A formal specification being correct and a product being correct are two different things. FSL checks within the range of the written rules. If the rules themselves are wrong, the verification results are based on those wrong rules too.

Also, in implementation-conformance testing, the quality of the Adapter matters. If `observe()` does not correctly reflect the implementation's state, a test may pass even when the implementation is broken. The Adapter may be generated by AI, but it should be treated as an object for human review.

Adopting FSL is not an activity of formalizing every specification. What matters is reducing the rules with high business and quality risk into a form that can be checked mechanically.

## Items to inquire about

When considering adoption, the items to confirm with stakeholders are as follows.

1. In the current development process, where are the areas where misaligned interpretation of specifications or missing exceptional cases are a problem?
2. Does a specification exist that can be used as an external standard against AI-generated implementations and tests?
3. As the first target to turn into FSL, can you choose one high-risk area such as payments, approvals, inventory, permissions, or asynchronous processing, or one piece of small state management such as screen transitions, unsaved changes, or double-submission prevention?
4. Can the existing acceptance criteria be expressed as state-transition scenarios?
5. Can the implementation side prepare the APIs, DB state, event logs, and test environment needed to connect an Adapter?
6. Can the FSL verification results be treated as input for CI or PR-review decisions?
7. When the specification changes, up to which of the business layer, requirements layer, design layer, and implementation tests will be in scope for updates?

## Proposed initial PoC

Limit the initial PoC to a scope that can be completed within two weeks.

Target of a lightweight PoC:

- Choose one screen or small feature whose state transitions are easy to understand
- Examples: unsaved-changes confirmation on an edit screen, double-submission prevention while submitting, retry after an error, operations by permission, back/cancel in a wizard

Deliverables of a lightweight PoC:

- A small `spec`
- The result of `fslc verify`
- A counterexample trace, or a `proved` result
- If needed, the scenario JSON from `fslc scenarios`
- A list of the detected UI-specification gaps, state-transition gaps, and QA perspectives

Target of an implementation-connected PoC:

- Choose one business flow with clear state transitions
- Examples: order cancellation, approval flow, inventory allocation, refund, permission change

Deliverables of an implementation-connected PoC:

- A `business` or `requirements` FSL specification
- An implementation-oriented `spec`
- The result of `fslc verify`
- The scenario JSON from `fslc scenarios`
- The pytest scaffold generated by `fslc testgen`
- A minimal Adapter connected to the implementation
- A list of the detected specification bugs, implementation bugs, and undefined requirements

Success conditions:

- You can discover ambiguous points or undefined exceptional cases from the existing specification
- For small state management, you can confirm state-transition gaps or unnecessary states as counterexamples
- You can turn the main acceptance criteria into scenarios
- For an implementation-connected PoC, you can run at least part of the implementation-conformance tests
- AI can read a counterexample trace and propose a fix
- Stakeholders can confirm value that is hard to obtain with ordinary specification documents and tests alone

## Conclusion

Formal methods is an approach for transforming a specification from human prose into a contract that can be checked mechanically. FSL is a tool for connecting that approach to AI-driven development and integrating business rules, requirements, design, implementation, and QA into a single verifiable flow.

The more AI raises development speed, the more important a mechanism becomes for mechanically checking the consistency of specifications and the conformance of implementations. FSL can be used as a vertically integrated verification harness for that purpose.
