# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""FSL grammar: Lark grammar string + AST transformer + parser instance."""
from lark import Lark, Transformer, v_args

# ---------------------------------------------------------------- grammar

GRAMMAR = r"""
start: top_def verify_def?
top_def: spec_def | refinement_def | compose_def | requirements_def | business_def | governance_def

verify_def: "verify" "{" verify_item* "}"
verify_item: "instances" NAME "=" INT ";"? -> verify_instances
           | "values" NAME "=" expr ".." expr ";"? -> verify_values

spec_def: "spec" NAME "{" item* "}"

compose_def: "compose" NAME "{" compose_item* "}"
?compose_item: use_def | internal_def | compose_state | compose_init
             | sync_action | action_def
             | invariant_def | trans_def | reachable_def | leadsto_def | until_def | unless_def
use_def: "use" NAME "as" NAME "from" STRING
internal_def: "internal" NAME "." NAME
compose_state: "state" "{" var_decl ("," var_decl)* ","? "}"
compose_init: "init" "{" stmt* "}"
sync_action: fair_sync_action | plain_sync_action
fair_sync_action: _FAIR "action" NAME "(" [compose_param ("," compose_param)*] ")" "=" sync_body meta_tag? "{" action_item* "}"
plain_sync_action: "action" NAME "(" [compose_param ("," compose_param)*] ")" "=" sync_body meta_tag? "{" action_item* "}"
sync_body: sync_ref ("||" sync_ref)*
sync_ref: NAME "." NAME "(" [expr ("," expr)*] ")"
compose_param: NAME ":" qname -> param_typed
             | NAME "in" expr ".." expr -> param_range
qname: NAME ("." NAME)?

refinement_def: "refinement" NAME "{" refinement_item* "}"
?refinement_item: refinement_impl | refinement_abs | maps_auto_def | map_def | refinement_action | preserve_progress_def
refinement_impl: "impl" NAME
refinement_abs: "abs" NAME
maps_auto_def: "maps" "auto"
map_def: "map" NAME ["[" binder "]"] "=" ref_expr
refinement_action: "action" NAME "(" [refinement_param ("," refinement_param)*] ")" "->" action_target
refinement_param: NAME [":" type]
action_target: stutter_target | mapped_action_target
stutter_target: "stutter"
mapped_action_target: NAME "(" [ref_expr ("," ref_expr)*] ")"
preserve_progress_def: "preserve" "progress" "{" progress_item* "}"
?progress_item: progress_respond
progress_respond: "respond" NAME "by" NAME ("," NAME)* ","?

?item: const_def | type_def | enum_def | struct_def | entity_def | number_def
     | state_def | init_def | action_def
     | invariant_def | trans_def | reachable_def | leadsto_def | until_def | unless_def | terminal_def

const_def: "const" NAME "=" expr
type_def: plain_type_def | symmetric_type_def
plain_type_def: "type" NAME "=" expr ".." expr
symmetric_type_def: "symmetric" "type" NAME "=" expr ".." expr
enum_def: plain_enum_def | symmetric_enum_def
plain_enum_def: "enum" NAME "{" enum_member ("," enum_member)* ","? "}"
symmetric_enum_def: "symmetric" "enum" NAME "{" enum_member ("," enum_member)* ","? "}"
enum_member: NAME -> enum_member
struct_def: "struct" NAME "{" field ("," field)* ","? "}"
field: NAME ":" type
entity_def: "entity" NAME
number_def: "number" NAME

state_def: "state" "{" var_decl ("," var_decl)* ","? "}"
var_decl: NAME ":" type
?type: "Int"  -> t_int
     | "Bool" -> t_bool
     | expr ".." expr -> t_range
     | "Map" "<" type "," type ">" -> t_map
     | "Set" "<" type ">" -> t_set
     | "Seq" "<" type "," cap ">" -> t_seq
     | "Option" "<" type ">" -> t_option
     | NAME -> t_name
cap: INT -> cap_int
    | NAME -> cap_name

init_def: "init" "{" stmt* "}"

action_def: fair_action | plain_action
fair_action: _FAIR "action" NAME "(" [param ("," param)*] ")" meta_tag? "{" action_item* "}"
plain_action: "action" NAME "(" [param ("," param)*] ")" meta_tag? "{" action_item* "}"
param: NAME ":" qname -> param_typed
     | NAME "in" expr ".." expr -> param_range
?action_item: requires_clause | ensures_clause | let_clause | stmt
requires_clause: "requires" expr
ensures_clause: "ensures" expr
let_clause: "let" NAME "=" expr

?stmt: assign | if_stmt | forall_stmt
assign: lvalue "=" expr
if_stmt: _IF expr "{" stmt_list "}" else_opt?
else_opt: _ELSE "{" stmt_list "}"
stmt_list: stmt*
forall_stmt: "forall" binder [":"] "{" stmt_list "}"
lvalue: NAME "[" expr "]" "." NAME -> lvalue_map_field
      | NAME "." NAME -> lvalue_field
      | NAME "[" expr "]" -> lvalue_index
      | NAME -> lvalue_var

binder: NAME ":" qname ["where" expr] -> binder_typed
       | NAME "in" expr ".." expr -> binder_range
       | NAME "in" expr ["where" expr] -> binder_collection

invariant_def: "invariant" NAME meta_tag? "{" expr "}"
trans_def: "trans" NAME meta_tag? "{" expr "}"
reachable_def: "reachable" NAME meta_tag? "{" expr "}"
terminal_def: "terminal" "{" expr "}"
until_def: "until" NAME meta_tag? "{" expr _UNTIL expr "}"
unless_def: "unless" NAME meta_tag? "{" expr _UNLESS expr "}"

leadsto_def: "leadsTo" NAME meta_tag? "{" lt_body leadsto_decreases? "}"
leadsto_decreases: "decreases" expr
meta_tag: STRING
?lt_body: lt_forall | lt_implies
lt_forall: "forall" binder [":"] "{" lt_body "}"
lt_implies: expr "~>" lt_target
?lt_target: _WITHIN expr expr -> lt_within
          | expr -> lt_target

?expr: quant | implies
quant: "forall" binder [":"] expr -> quant_forall
     | "forall" binder [":"] "{" expr "}" -> quant_forall_brace
     | "exists" binder [":"] expr -> quant_exists
     | "exists" binder [":"] "{" expr "}" -> quant_exists_brace
?implies: or_e | or_e "=>" implies -> imp
?or_e: and_e | or_e _OR and_e -> or_op
?and_e: not_e | and_e _AND not_e -> and_op
?not_e: _NOT not_e -> not_op
       | is_e
?is_e: cmp ["is" pattern] -> is_pat
pattern: "none" -> pat_none
       | "some" "(" NAME ")" -> pat_some
?cmp: sum | sum CMPOP sum -> cmp_op
?sum: product | sum "+" product -> add | sum "-" product -> sub
?product: unary | product "*" unary -> mul | product "/" unary -> div | product "%" unary -> mod
?unary: "-" unary -> neg | postfix
postfix: atom postfix_suffix*
postfix_suffix: "[" expr "]" -> idx_suffix
               | "." "contains" "(" [expr_list] ")" -> method_contains
               | "." "add" "(" [expr_list] ")" -> method_add
               | "." "remove" "(" [expr_list] ")" -> method_remove
               | "." "push" "(" [expr_list] ")" -> method_push
               | "." "pop" "(" ")" -> method_pop
               | "." "head" "(" ")" -> method_head
               | "." "at" "(" expr ")" -> method_at
               | "." "size" "(" ")" -> method_size
               | "." NAME -> field_suffix
?atom: INT -> num
     | "true" -> true_lit
     | "false" -> false_lit
     | "none" -> none_lit
     | "some" "(" expr ")" -> some_lit
     | "Set" "{" [expr_list] "}" -> set_lit
     | "Seq" "{" [expr_list] "}" -> seq_lit
     | NAME struct_fields -> struct_lit
     | "stage" "(" expr ")" -> stage_e
     | "count" "(" NAME ":" qname "where" expr ")" -> count_e
     | "sum" "(" NAME ":" qname "of" expr ["where" expr] ")" -> sum_e
     | "min" "(" expr "," expr ")" -> min_e
     | "max" "(" expr "," expr ")" -> max_e
     | "abs" "(" expr ")" -> abs_e
     | "old" "(" expr ")" -> old_e
     | "unique" "(" binder ")" -> unique_e
     | "exactlyOne" "(" binder ")" -> exactly_one_e
     | NAME -> var
     | "(" expr ")"
struct_fields: "{" NAME ":" expr ("," NAME ":" expr)* ","? "}"
expr_list: expr ("," expr)*

?ref_expr: _IF ref_expr "then" ref_expr _ELSE ref_expr -> ite
         | NAME ref_struct_fields -> struct_lit
         | expr
ref_struct_fields: "{" NAME ":" ref_expr ("," NAME ":" ref_expr)* ","? "}" -> struct_fields

requirements_def: "requirements" NAME "{" requirements_item* "}"
?requirements_item: implements_def | requirement_def | acceptance_def | forbidden_def | kpi_def
                  | const_def | type_def | enum_def | struct_def | entity_def | number_def
                  | state_def | init_def | req_action_def | process_def | time_def
                  | invariant_def | trans_def | reachable_def | leadsto_def | until_def | unless_def
implements_def: "implements" NAME "from" STRING "{" implements_item* "}"
?implements_item: map_def | maps_auto_def | preserve_progress_def
requirement_def: "requirement" REQ_ID STRING "{" requirement_item* "}"
?requirement_item: req_action_def | invariant_def | trans_def | reachable_def | leadsto_def | deadline_def
req_action_def: req_fair_action | req_plain_action
req_fair_action: _FAIR "action" NAME "(" [param ("," param)*] ")" maps_clause? meta_tag? "{" req_action_item* "}"
req_plain_action: "action" NAME "(" [param ("," param)*] ")" maps_clause? meta_tag? "{" req_action_item* "}"
?req_action_item: requires_clause | ensures_clause | let_clause | branches_def | stmt
branches_def: "branches" "{" branch_when+ "}"
branch_when: "when" expr "{" stmt* "}" maps_clause
maps_clause: "maps" req_action_target
req_action_target: stutter_target | req_mapped_action_target
req_mapped_action_target: NAME "(" [ref_expr ("," ref_expr)*] ")"
acceptance_def: "acceptance" REQ_ID STRING "{" acceptance_step* acceptance_expect "}"
acceptance_step: NAME "(" [acceptance_arg ("," acceptance_arg)*] ")"
acceptance_arg: ref_expr
acceptance_expect: "expect" expr -> acceptance_expect
                 | "expect" NAME INT "in" NAME -> acceptance_expect_stage
forbidden_def: "forbidden" REQ_ID STRING "{" acceptance_step* "expect" "rejected" "}"
time_def: "time" "{" time_item* "}"
?time_item: urgent_def | age_def
urgent_def: "urgent" NAME ("," NAME)* ","?
age_def: "age" NAME ["[" binder "]"] "while" expr
deadline_def: "deadline" NAME "<=" expr

business_def: "business" NAME "{" business_item* "}"
?business_item: actor_def | entity_def | process_def | kpi_def | control_def | policy_def | goal_def
actor_def: "actor" NAME ("," NAME)* ","?
process_def: "process" NAME process_with? "{" process_item* "}"
process_with: "with" proc_field ("," proc_field)*
proc_field: NAME ":" qname ["=" expr]
?process_item: process_stages | process_initial | process_transition
process_stages: "stages" NAME ("," NAME)* ","?
process_initial: "initial" NAME
process_transition: "transition" NAME NAME "->" NAME "by" NAME trans_input? trans_guard? trans_set? trans_covers?
trans_input: "with" param ("," param)*
trans_guard: "when" expr
trans_set: "set" proc_assign ("," proc_assign)*
proc_assign: NAME "=" expr
trans_covers: "covers" REQ_ID STRING
kpi_def: "kpi" NAME "=" "count" NAME "in" NAME
control_def: "control" REQ_ID STRING control_attr*
?control_attr: control_owner | control_severity | control_applies_to
control_owner: "owner" NAME
control_severity: "severity" NAME
control_applies_to: "applies_to" NAME
satisfies_clause: "satisfies" REQ_ID ("," REQ_ID)* ","?
policy_def: "policy" REQ_ID STRING satisfies_clause? policy_body
?policy_body: policy_invariant | policy_responds | policy_eventually
policy_invariant: "invariant" "{" expr "}"
policy_responds: "responds" "{" lt_body "}"
policy_eventually: "every" NAME "in" NAME "must" "eventually" "be" stage_disjunction
goal_def: "goal" REQ_ID STRING satisfies_clause? goal_body
?goal_body: goal_expr | goal_some_stage | goal_all_stage
goal_expr: "{" expr "}"
goal_some_stage: "some" NAME "can" "reach" NAME
goal_all_stage: "all" NAME "can" "be" stage_disjunction
stage_disjunction: NAME (_OR NAME)*

governance_def: "governance" NAME "{" governance_item* "}"
?governance_item: governance_authority | control_def | governance_delegates | governance_preservation
governance_authority: "authority" NAME "owns" REQ_ID ("," REQ_ID)* ","?
governance_delegates: "delegates" NAME "from" STRING "{" governance_delegate_item* "}"
?governance_delegate_item: governance_require | governance_satisfaction
governance_require: "require" REQ_ID
governance_satisfaction: REQ_ID "is" "satisfied_by" governance_artifact_ref ("," governance_artifact_ref)* ","?
?governance_artifact_ref: governance_policy_ref | governance_goal_ref
governance_policy_ref: "policy" REQ_ID
governance_goal_ref: "goal" REQ_ID
governance_preservation: "preservation" NAME "{" preservation_item* "}"
?preservation_item: preservation_before | preservation_after | preservation_preserve | preservation_refinement
preservation_before: "before" NAME "from" STRING
preservation_after: "after" NAME "from" STRING
preservation_preserve: "preserve" REQ_ID
preservation_refinement: "checked_by" "refinement" STRING

CMPOP: "==" | "!=" | "<=" | ">=" | "<" | ">"
REQ_ID: /[A-Za-z0-9]+(?:[-_][A-Za-z0-9]+)*/
_AND: /and\b/
_OR: /or\b/
_NOT: /not\b/
_IF: /if\b/
_ELSE: /else\b/
_FAIR: /fair\b/
_WITHIN: /within\b/
_UNTIL: /until\b/
_UNLESS: /unless\b/
NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
INT: /[0-9]+/
STRING: /"[^"]*"/
COMMENT: /\/\/[^\n]*/
%import common.WS
%ignore WS
%ignore COMMENT
"""


def _args(xs):
    # maybe_placeholders=True gives (None,) for empty arg lists; strip the sentinel.
    return [x for x in xs if x is not None]


def _loc(meta):
    if meta is None:
        return None
    return {"line": meta.line, "column": meta.column}


def _flatten_leadsto(body):
    binders = []
    node = body
    while node[0] == "lt_forall":
        _, binder, inner = node
        binders.append(binder)
        node = inner
    if node[0] != "lt_implies":
        raise ValueError(f"expected leadsTo implication, got {node[0]}")
    _, p, q, within = node
    return binders, p, q, within


def _parse_meta(s):
    if ":" in s:
        ident, text = s.split(":", 1)
        return {"id": ident.strip(), "text": text.strip()}
    return {"id": s.strip(), "text": None}


@v_args(inline=True, meta=True)
class Ast(Transformer):
    def NAME(self, *args):
        # With meta=True, rule nodes get (meta, ...); terminals pass only the token.
        return str(args[-1])

    def num(self, meta, n):
        return ("num", int(n))

    def true_lit(self, meta):
        return ("bool", True)

    def false_lit(self, meta):
        return ("bool", False)

    def none_lit(self, meta):
        return ("none",)

    def some_lit(self, meta, e):
        return ("some", e)

    def set_lit(self, meta, items=None):
        return ("set_lit", list(items or []))

    def seq_lit(self, meta, items=None):
        return ("seq_lit", list(items or []))

    def struct_lit(self, meta, name, fields):
        return ("struct_lit", name, dict(fields))

    def struct_fields(self, meta, *pairs):
        out = []
        for i in range(0, len(pairs), 2):
            out.append((pairs[i], pairs[i + 1]))
        return out

    def var(self, meta, n):
        return ("var", n)

    def idx_suffix(self, meta, e):
        return ("idx", e)

    def field_suffix(self, meta, name):
        return ("field", name)

    def expr_list(self, meta, *exprs):
        return list(exprs)

    def _method_suffix(self, name, args=None):
        return ("method", name, list(args or []))

    def method_contains(self, meta, args=None):
        return self._method_suffix("contains", args)

    def method_add(self, meta, args=None):
        return self._method_suffix("add", args)

    def method_remove(self, meta, args=None):
        return self._method_suffix("remove", args)

    def method_size(self, meta):
        return self._method_suffix("size", [])

    def method_push(self, meta, args=None):
        return self._method_suffix("push", args)

    def method_pop(self, meta):
        return self._method_suffix("pop", [])

    def method_head(self, meta):
        return self._method_suffix("head", [])

    def method_at(self, meta, idx):
        return self._method_suffix("at", [idx])

    def cap_int(self, meta, n):
        return ("num", int(n))

    def cap_name(self, meta, n):
        return ("var", str(n))

    def t_seq(self, meta, elem, cap):
        return ("seq", elem, cap)

    def postfix(self, meta, atom, *suffixes):
        e = atom
        for s in suffixes:
            if s[0] == "idx":
                e = ("index", e, s[1])
            elif s[0] == "field":
                e = ("field", e, s[1])
            elif s[0] == "method":
                e = ("method", e, s[1], s[2])
        return e

    def add(self, meta, a, b):
        return ("bin", "+", a, b)

    def sub(self, meta, a, b):
        return ("bin", "-", a, b)

    def mul(self, meta, a, b):
        return ("bin", "*", a, b)

    def div(self, meta, a, b):
        return ("bin", "/", a, b)

    def mod(self, meta, a, b):
        return ("bin", "%", a, b)

    def neg(self, meta, a):
        return ("neg", a)

    def cmp_op(self, meta, a, op, b):
        return ("bin", str(op), a, b)

    def and_op(self, meta, a, b):
        return ("bin", "and", a, b)

    def or_op(self, meta, a, b):
        return ("bin", "or", a, b)

    def imp(self, meta, a, b):
        return ("bin", "=>", a, b)

    def not_op(self, meta, a):
        return ("not", a)

    def ite(self, meta, c, a, b):
        return ("ite", c, a, b)

    def is_pat(self, meta, a, pat=None):
        if pat is None:
            return a
        return ("is", a, pat)

    def pat_none(self, meta):
        return ("pat_none",)

    def pat_some(self, meta, name):
        return ("pat_some", name)

    def count_e(self, meta, v, ty, cond):
        return ("count", v, ty, cond)

    def sum_e(self, meta, v, ty, body, cond=None):
        return ("sum", v, ty, body, cond)

    def min_e(self, meta, a, b):
        return ("min", a, b)

    def max_e(self, meta, a, b):
        return ("max", a, b)

    def abs_e(self, meta, a):
        return ("abs", a)

    def old_e(self, meta, e):
        return ("old", e)

    def stage_e(self, meta, e):
        return ("stage", e, _loc(meta))

    def binder_typed(self, meta, v, ty, where=None):
        return ("binder_typed", v, ty, where)

    def binder_range(self, meta, v, lo, hi):
        return ("binder_range", v, lo, hi)

    def binder_collection(self, meta, v, collection, where=None):
        return ("binder_collection", v, collection, where)

    def quant_forall(self, meta, b, e):
        return ("forall", b, e)

    def quant_forall_brace(self, meta, b, e):
        return ("forall", b, e)

    def quant_exists(self, meta, b, e):
        return ("exists", b, e)

    def quant_exists_brace(self, meta, b, e):
        return ("exists", b, e)

    def unique_e(self, meta, binder):
        return ("unique", binder)

    def exactly_one_e(self, meta, binder):
        return ("exactly_one", binder)

    def lvalue_var(self, meta, n):
        return ("var", n)

    def lvalue_index(self, meta, n, idx):
        return ("index", n, idx)

    def lvalue_field(self, meta, n, field):
        return ("field_lv", ("var", n), field)

    def lvalue_map_field(self, meta, n, idx, field):
        return ("field_lv", ("index", n, idx), field)

    def assign(self, meta, lv, e):
        return ("assign", lv, e, _loc(meta))

    def stmt_list(self, meta, *stmts):
        return [s for s in stmts if s is not None]

    def else_opt(self, meta, stmts):
        return stmts

    def if_stmt(self, meta, cond, then_stmts, else_stmts=None):
        return ("if", cond, then_stmts, else_stmts or [], _loc(meta))

    def forall_stmt(self, meta, b, stmts):
        return ("forall_stmt", b, stmts, _loc(meta))

    def requires_clause(self, meta, e):
        return ("requires", e, _loc(meta))

    def ensures_clause(self, meta, e):
        return ("ensures", e, _loc(meta))

    def let_clause(self, meta, n, e):
        return ("let", n, e, _loc(meta))

    def param_typed(self, meta, n, ty):
        return ("param_typed", n, ty)

    def param_range(self, meta, n, lo, hi):
        return ("param_range", n, lo, hi)

    def t_int(self, meta):
        return ("int",)

    def t_bool(self, meta):
        return ("bool",)

    def t_range(self, meta, lo, hi):
        return ("range", lo, hi)

    def t_map(self, meta, k, v):
        return ("map", k, v)

    def t_set(self, meta, elem):
        return ("set", elem)

    def t_option(self, meta, inner):
        return ("option", inner)

    def t_name(self, meta, n):
        return ("name", n)

    def var_decl(self, meta, n, ty):
        return ("decl", n, ty)

    def entity_def(self, meta, name):
        return ("entity", name, _loc(meta))

    def number_def(self, meta, name):
        return ("number", name, _loc(meta))

    def enum_member(self, meta, name):
        return name

    def field(self, meta, n, ty):
        return (n, ty)

    def state_def(self, meta, *decls):
        return ("state", [d for d in decls if d])

    def const_def(self, meta, n, e):
        return ("const", n, e)

    def plain_type_def(self, meta, n, lo, hi):
        return ("type", n, lo, hi)

    def symmetric_type_def(self, meta, n, lo, hi):
        return ("type", n, lo, hi, {"symmetric": True})

    def type_def(self, meta, node):
        return node

    def plain_enum_def(self, meta, n, *members):
        return ("enum", n, [m for m in members if m])

    def symmetric_enum_def(self, meta, n, *members):
        return ("enum", n, [m for m in members if m], {"symmetric": True})

    def enum_def(self, meta, node):
        return node

    def struct_def(self, meta, n, *fields):
        return ("struct", n, dict(fields))

    def init_def(self, meta, *stmts):
        return ("init", list(stmts))

    def meta_tag(self, meta, s):
        return _parse_meta(s)

    def invariant_def(self, meta, n, *rest):
        req_meta, e = None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            else:
                e = r
        return ("invariant", n, e, _loc(meta), req_meta)

    def trans_def(self, meta, n, *rest):
        req_meta, e = None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            else:
                e = r
        return ("trans", n, e, _loc(meta), req_meta)

    def reachable_def(self, meta, n, *rest):
        req_meta, e = None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            else:
                e = r
        return ("reachable", n, e, _loc(meta), req_meta)

    def terminal_def(self, meta, e):
        return ("terminal", e, _loc(meta))

    def until_def(self, meta, n, *rest):
        req_meta, p, q = None, None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            elif p is None:
                p = r
            else:
                q = r
        return ("until", n, p, q, _loc(meta), req_meta)

    def unless_def(self, meta, n, *rest):
        req_meta, p, q = None, None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            elif p is None:
                p = r
            else:
                q = r
        return ("unless", n, p, q, _loc(meta), req_meta)

    def _action_parts(self, meta, name, *rest):
        params, items, req_meta = [], [], None
        for r in rest:
            if r is None:
                continue
            if isinstance(r, dict):
                req_meta = r
            elif r[0] in ("param_typed", "param_range"):
                params.append(r)
            else:
                items.append(r)
        return ("action", name, params, items, _loc(meta), req_meta)

    def fair_action(self, meta, name, *rest):
        base = self._action_parts(meta, name, *rest)
        return base[:5] + (True, base[5])

    def plain_action(self, meta, name, *rest):
        base = self._action_parts(meta, name, *rest)
        return base[:5] + (False, base[5])

    def action_def(self, meta, node):
        return node

    def maps_clause(self, meta, target):
        return ("maps", target, _loc(meta))

    def req_mapped_action_target(self, meta, name, *exprs):
        return ("action", name, _args(exprs))

    def req_action_target(self, meta, child):
        return child

    def branch_when(self, meta, cond, *parts):
        stmts = []
        maps = None
        for p in parts:
            if isinstance(p, tuple) and p[0] == "maps":
                maps = p
            else:
                stmts.append(p)
        return ("branch", cond, stmts, maps, _loc(meta))

    def branches_def(self, meta, *branches):
        return ("branches", list(branches), _loc(meta))

    def _req_action_parts(self, meta, name, *rest):
        params, items, req_meta, maps = [], [], None, None
        for r in rest:
            if r is None:
                continue
            if isinstance(r, dict):
                req_meta = r
            elif isinstance(r, tuple) and r[0] in ("param_typed", "param_range"):
                params.append(r)
            elif isinstance(r, tuple) and r[0] == "maps":
                maps = r
            else:
                items.append(r)
        return ("req_action", name, params, items, _loc(meta), False, req_meta, maps)

    def req_fair_action(self, meta, name, *rest):
        base = self._req_action_parts(meta, name, *rest)
        return base[:5] + (True, base[6], base[7])

    def req_plain_action(self, meta, name, *rest):
        return self._req_action_parts(meta, name, *rest)

    def req_action_def(self, meta, node):
        return node

    def lt_target(self, meta, q):
        return ("lt_target", q, None)

    def lt_within(self, meta, bound, q):
        return ("lt_target", q, bound)

    def lt_implies(self, meta, p, target):
        _, q, within = target
        return ("lt_implies", p, q, within)

    def lt_forall(self, meta, binder, body):
        return ("lt_forall", binder, body)

    def leadsto_decreases(self, meta, measure):
        return ("decreases", measure)

    def leadsto_def(self, meta, name, *rest):
        req_meta, body, measure = None, None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            elif isinstance(r, tuple) and r[0] == "decreases":
                measure = r[1]
            else:
                body = r
        binders, p, q, within = _flatten_leadsto(body)
        return ("leadsto", name, binders, p, q, _loc(meta), req_meta, measure, within)

    def top_def(self, meta, child):
        return child

    def verify_instances(self, meta, name, n):
        return ("verify_instances", name, int(n), _loc(meta))

    def verify_values(self, meta, name, lo, hi):
        return ("verify_values", name, lo, hi, _loc(meta))

    def verify_def(self, meta, *items):
        return ("verify_bounds", [i for i in items if i], _loc(meta))

    def start(self, meta, child, verify=None):
        if verify is not None and child[0] in ("spec", "business", "requirements"):
            tag, name, items = child
            return (tag, name, items + [verify])
        return child

    def spec_def(self, meta, name, *items):
        return ("spec", name, [i for i in items if i])

    def refinement_impl(self, meta, name):
        return ("impl", name)

    def refinement_abs(self, meta, name):
        return ("abs", name)

    def maps_auto_def(self, meta):
        return ("maps_auto", _loc(meta))

    def map_def(self, meta, name, binder=None, expr=None):
        if binder is not None:
            return ("map", name, binder, expr, _loc(meta))
        return ("map", name, None, expr, _loc(meta))

    def refinement_param(self, meta, name, ty=None):
        return ("refinement_param", name, ty)

    def action_target(self, meta, child):
        return child

    def stutter_target(self, meta):
        return ("stutter",)

    def mapped_action_target(self, meta, name, *exprs):
        return ("action", name, _args(exprs))

    def refinement_action(self, meta, name, *params_and_target):
        params = []
        target = None
        for p in params_and_target:
            if isinstance(p, tuple) and p[0] == "refinement_param":
                params.append(p)
            else:
                target = p
        if target is None:
            raise ValueError("refinement action missing target")
        return ("action_map", name, params, target, _loc(meta))

    def progress_respond(self, meta, leadsto_name, *action_names):
        return ("progress_respond", leadsto_name, list(action_names), _loc(meta))

    def preserve_progress_def(self, meta, *items):
        return ("preserve_progress", [i for i in items if i], _loc(meta))

    def refinement_def(self, meta, name, *items):
        return ("refinement", name, [i for i in items if i])

    def STRING(self, *args):
        s = str(args[-1])
        return s[1:-1]

    def qname(self, meta, first, second=None):
        if second is None:
            return first
        return ("qname", first, second)

    def use_def(self, meta, spec_name, alias, path):
        return ("use", spec_name, alias, path, _loc(meta))

    def internal_def(self, meta, alias, action):
        return ("internal", alias, action, _loc(meta))

    def compose_state(self, meta, *decls):
        return ("state", [d for d in decls if d])

    def compose_init(self, meta, *stmts):
        return ("init", list(stmts))

    def sync_ref(self, meta, alias, action, *args):
        return ("sync_ref", alias, action, list(args))

    def sync_body(self, meta, *refs):
        return list(refs)

    def _sync_action_parts(self, meta, fair, name, *rest):
        params, sync_refs, body_items, req_meta = [], None, [], None
        for r in rest:
            if r is None:
                continue
            if isinstance(r, dict):
                req_meta = r
            elif isinstance(r, tuple) and r[0] in ("param_typed", "param_range"):
                params.append(r)
            elif isinstance(r, list) and r and isinstance(r[0], tuple) and r[0][0] == "sync_ref":
                sync_refs = r
            elif isinstance(r, tuple) and r[0] in (
                "requires", "ensures", "let", "assign", "if", "forall_stmt",
            ):
                body_items.append(r)
            elif not isinstance(r, (tuple, list)):
                continue
            elif isinstance(r, tuple) and r[0] == "sync_ref":
                sync_refs = [r]
        if sync_refs is None:
            raise ValueError("sync action missing sync body")
        return ("sync_action", name, params, sync_refs, body_items, _loc(meta), fair, req_meta)

    def fair_sync_action(self, meta, name, *rest):
        return self._sync_action_parts(meta, True, name, *rest)

    def plain_sync_action(self, meta, name, *rest):
        return self._sync_action_parts(meta, False, name, *rest)

    def sync_action(self, meta, node):
        return node

    def compose_def(self, meta, name, *items):
        return ("compose", name, [i for i in items if i])

    def requirement_def(self, meta, req_id, text, *items):
        return ("requirement", str(req_id), text, [i for i in items if i], _loc(meta))

    def implements_def(self, meta, name, path, *items):
        return ("implements", name, path, [i for i in items if i], _loc(meta))

    def requirements_def(self, meta, name, *items):
        return ("requirements", name, [i for i in items if i])

    def acceptance_arg(self, meta, expr):
        return expr

    def acceptance_step(self, meta, name, *args):
        return ("acceptance_step", name, _args(args), _loc(meta))

    def acceptance_expect(self, meta, expr):
        return ("acceptance_expect", expr, _loc(meta))

    def acceptance_expect_stage(self, meta, entity, n, stage):
        return ("acceptance_expect_stage", entity, int(n), stage, _loc(meta))

    def acceptance_def(self, meta, ac_id, text, *parts):
        steps = []
        expect = None
        for p in parts:
            if isinstance(p, tuple) and p[0] == "acceptance_step":
                steps.append(p)
            elif isinstance(p, tuple) and p[0] in ("acceptance_expect", "acceptance_expect_stage"):
                expect = p
        return ("acceptance", str(ac_id), text, steps, expect, _loc(meta))

    def forbidden_def(self, meta, fb_id, text, *parts):
        # `expect rejected` is an inline marker (no tree node); parts are the steps.
        steps = [p for p in parts if isinstance(p, tuple) and p[0] == "acceptance_step"]
        return ("forbidden", str(fb_id), text, steps, _loc(meta))

    def urgent_def(self, meta, *names):
        return ("time_urgent", list(names), _loc(meta))

    def age_def(self, meta, name, binder=None, cond=None):
        if cond is None:
            cond = binder
            binder = None
        return ("time_age", name, binder, cond, _loc(meta))

    def time_def(self, meta, *items):
        return ("time", [i for i in items if i], _loc(meta))

    def deadline_def(self, meta, name, bound):
        return ("deadline", name, bound, _loc(meta))

    def actor_def(self, meta, *names):
        return ("biz_actor", list(names), _loc(meta))

    def process_stages(self, meta, *names):
        return ("biz_stages", list(names), _loc(meta))

    def process_initial(self, meta, name):
        return ("biz_initial", name, _loc(meta))

    def proc_field(self, meta, n, ty, init=None):
        return ("proc_field", n, ty, init)

    def process_with(self, meta, *fields):
        return ("proc_fields", list(fields), _loc(meta))

    def trans_input(self, meta, *params):
        return ("trans_input", list(params))

    def trans_guard(self, meta, e):
        return ("trans_guard", e)

    def proc_assign(self, meta, n, e):
        return ("proc_assign", n, e)

    def trans_set(self, meta, *assigns):
        return ("trans_set", list(assigns))

    def trans_covers(self, meta, req_id, text):
        return ("trans_covers", str(req_id), text)

    def process_transition(self, meta, name, src, dst, actor, *extras):
        extra = {"inputs": [], "guard": None, "sets": [], "covers": None}
        for item in extras:
            if item is None:
                continue
            if item[0] == "trans_input":
                extra["inputs"] = item[1]
            elif item[0] == "trans_guard":
                extra["guard"] = item[1]
            elif item[0] == "trans_set":
                extra["sets"] = item[1]
            elif item[0] == "trans_covers":
                extra["covers"] = (item[1], item[2])
        if not any(extra.values()):
            extra = {}
        return ("biz_transition", name, src, dst, actor, extra, _loc(meta))

    def process_def(self, meta, name, *rest):
        fields = None
        items = list(rest)
        if items and isinstance(items[0], tuple) and items[0][0] == "proc_fields":
            fields = items[0]
            items = items[1:]
        return ("biz_process", name, fields, [i for i in items if i], _loc(meta))

    def kpi_def(self, meta, name, case_name, stage):
        return ("biz_kpi", name, case_name, stage, _loc(meta))

    def policy_invariant(self, meta, expr):
        return ("biz_policy_invariant", expr)

    def policy_responds(self, meta, body):
        binders, p, q, within = _flatten_leadsto(body)
        return ("biz_policy_responds", binders, p, q, within)

    def stage_disjunction(self, meta, *names):
        return list(names)

    def policy_eventually(self, meta, case_name, source_stage, target_stages):
        return ("biz_policy_eventually", case_name, source_stage, target_stages)

    def control_owner(self, meta, name):
        return ("control_owner", name)

    def control_severity(self, meta, name):
        return ("control_severity", name)

    def control_applies_to(self, meta, name):
        return ("control_applies_to", name)

    def control_def(self, meta, control_id, text, *attrs):
        return ("biz_control", str(control_id), text, [a for a in attrs if a], _loc(meta))

    def satisfies_clause(self, meta, *control_ids):
        return ("satisfies", [str(cid) for cid in control_ids], _loc(meta))

    def policy_def(self, meta, policy_id, text, *parts):
        satisfies = []
        body = None
        for part in parts:
            if isinstance(part, tuple) and part[0] == "satisfies":
                satisfies = part[1]
            else:
                body = part
        return ("biz_policy", str(policy_id), text, body, _loc(meta), satisfies)

    def goal_expr(self, meta, expr):
        return ("biz_goal_expr", expr)

    def goal_some_stage(self, meta, case_name, stage):
        return ("biz_goal_some_stage", case_name, stage)

    def goal_all_stage(self, meta, case_name, stages):
        return ("biz_goal_all_stage", case_name, stages)

    def goal_def(self, meta, goal_id, text, *parts):
        satisfies = []
        body = None
        for part in parts:
            if isinstance(part, tuple) and part[0] == "satisfies":
                satisfies = part[1]
            else:
                body = part
        return ("biz_goal", str(goal_id), text, body, _loc(meta), satisfies)

    def business_def(self, meta, name, *items):
        return ("business", name, [i for i in items if i])

    def governance_authority(self, meta, authority, *control_ids):
        return ("gov_authority", authority, [str(cid) for cid in control_ids], _loc(meta))

    def governance_require(self, meta, control_id):
        return ("gov_require", str(control_id), _loc(meta))

    def governance_policy_ref(self, meta, policy_id):
        return ("policy", str(policy_id), _loc(meta))

    def governance_goal_ref(self, meta, goal_id):
        return ("goal", str(goal_id), _loc(meta))

    def governance_satisfaction(self, meta, control_id, *policy_refs):
        return ("gov_satisfaction", str(control_id), [p for p in policy_refs if p], _loc(meta))

    def governance_delegates(self, meta, business_name, path, *items):
        return ("gov_delegates", business_name, path, [i for i in items if i], _loc(meta))

    def preservation_before(self, meta, spec_name, path):
        return ("preservation_before", spec_name, path, _loc(meta))

    def preservation_after(self, meta, spec_name, path):
        return ("preservation_after", spec_name, path, _loc(meta))

    def preservation_preserve(self, meta, control_id):
        return ("preservation_preserve", str(control_id), _loc(meta))

    def preservation_refinement(self, meta, path):
        return ("preservation_refinement", path, _loc(meta))

    def governance_preservation(self, meta, name, *items):
        return ("gov_preservation", name, [i for i in items if i], _loc(meta))

    def governance_def(self, meta, name, *items):
        return ("governance", name, [i for i in items if i])


PARSER = Lark(
    GRAMMAR,
    parser="earley",
    maybe_placeholders=True,
    propagate_positions=True,
)
