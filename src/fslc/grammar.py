"""FSL grammar: Lark grammar string + AST transformer + parser instance."""
from lark import Lark, Transformer, v_args

# ---------------------------------------------------------------- grammar

GRAMMAR = r"""
start: spec_def | refinement_def | compose_def | requirements_def

spec_def: "spec" NAME "{" item* "}"

compose_def: "compose" NAME "{" compose_item* "}"
?compose_item: use_def | internal_def | compose_state | compose_init
             | sync_action | action_def
             | invariant_def | reachable_def | leadsto_def
use_def: "use" NAME "as" NAME "from" STRING
internal_def: "internal" NAME "." NAME
compose_state: "state" "{" var_decl ("," var_decl)* ","? "}"
compose_init: "init" "{" stmt* "}"
sync_action: fair_sync_action | plain_sync_action
fair_sync_action: _FAIR "action" NAME "(" [compose_param ("," compose_param)*] ")" "=" sync_body meta_tag? "{" action_item* "}"
plain_sync_action: "action" NAME "(" [compose_param ("," compose_param)*] ")" "=" sync_body meta_tag? "{" action_item* "}"
glue_action: fair_action | plain_action
sync_body: sync_ref ("||" sync_ref)*
sync_ref: NAME "." NAME "(" [expr ("," expr)*] ")"
compose_param: NAME ":" qname -> param_typed
             | NAME "in" expr ".." expr -> param_range
qname: NAME ("." NAME)?

refinement_def: "refinement" NAME "{" refinement_item* "}"
?refinement_item: refinement_impl | refinement_abs | map_def | refinement_action
refinement_impl: "impl" NAME
refinement_abs: "abs" NAME
map_def: "map" NAME ["[" binder "]"] "=" ref_expr
refinement_action: "action" NAME "(" [refinement_param ("," refinement_param)*] ")" "->" action_target
refinement_param: NAME
action_target: stutter_target | mapped_action_target
stutter_target: "stutter"
mapped_action_target: NAME "(" [ref_expr ("," ref_expr)*] ")"

?item: const_def | type_def | enum_def | struct_def
     | state_def | init_def | action_def
     | invariant_def | reachable_def | leadsto_def

const_def: "const" NAME "=" expr
type_def: "type" NAME "=" expr ".." expr
enum_def: "enum" NAME "{" enum_member ("," enum_member)* ","? "}"
enum_member: NAME -> enum_member
struct_def: "struct" NAME "{" field ("," field)* ","? "}"
field: NAME ":" type

state_def: "state" "{" var_decl ("," var_decl)* ","? "}"
var_decl: NAME ":" type
?type: "Int"  -> t_int
     | "Bool" -> t_bool
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

invariant_def: "invariant" NAME meta_tag? "{" expr "}"
reachable_def: "reachable" NAME meta_tag? "{" expr "}"

leadsto_def: "leadsTo" NAME meta_tag? "{" lt_body "}"
meta_tag: STRING
?lt_body: lt_forall | lt_implies
lt_forall: "forall" binder [":"] "{" lt_body "}"
lt_implies: expr "~>" expr

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
?product: unary | product "*" unary -> mul
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
     | "count" "(" NAME ":" qname "where" expr ")" -> count_e
     | "sum" "(" NAME ":" qname "of" expr ["where" expr] ")" -> sum_e
     | "min" "(" expr "," expr ")" -> min_e
     | "max" "(" expr "," expr ")" -> max_e
     | "abs" "(" expr ")" -> abs_e
     | "old" "(" expr ")" -> old_e
     | NAME -> var
     | "(" expr ")"
struct_fields: "{" NAME ":" expr ("," NAME ":" expr)* ","? "}"
expr_list: expr ("," expr)*

?ref_expr: _IF ref_expr "then" ref_expr _ELSE ref_expr -> ite
         | ref_quant
         | ref_implies
ref_quant: "forall" binder [":"] ref_expr -> quant_forall
         | "forall" binder [":"] "{" ref_expr "}" -> quant_forall_brace
         | "exists" binder [":"] ref_expr -> quant_exists
         | "exists" binder [":"] "{" ref_expr "}" -> quant_exists_brace
?ref_implies: ref_or_e | ref_or_e "=>" ref_implies -> imp
?ref_or_e: ref_and_e | ref_or_e _OR ref_and_e -> or_op
?ref_and_e: ref_not_e | ref_and_e _AND ref_not_e -> and_op
?ref_not_e: _NOT ref_not_e -> not_op
          | ref_is_e
?ref_is_e: ref_cmp ["is" pattern] -> is_pat
?ref_cmp: ref_sum | ref_sum CMPOP ref_sum -> cmp_op
?ref_sum: ref_product | ref_sum "+" ref_product -> add | ref_sum "-" ref_product -> sub
?ref_product: ref_unary | ref_product "*" ref_unary -> mul
?ref_unary: "-" ref_unary -> neg | ref_postfix
ref_postfix: ref_atom ref_postfix_suffix* -> postfix
ref_postfix_suffix: "[" ref_expr "]" -> idx_suffix
                  | "." "contains" "(" [ref_expr_list] ")" -> method_contains
                  | "." "add" "(" [ref_expr_list] ")" -> method_add
                  | "." "remove" "(" [ref_expr_list] ")" -> method_remove
                  | "." "push" "(" [ref_expr_list] ")" -> method_push
                  | "." "pop" "(" ")" -> method_pop
                  | "." "head" "(" ")" -> method_head
                  | "." "at" "(" ref_expr ")" -> method_at
                  | "." "size" "(" ")" -> method_size
                  | "." NAME -> field_suffix
?ref_atom: INT -> num
         | "true" -> true_lit
         | "false" -> false_lit
         | "none" -> none_lit
         | "some" "(" ref_expr ")" -> some_lit
         | "Set" "{" [ref_expr_list] "}" -> set_lit
         | "Seq" "{" [ref_expr_list] "}" -> seq_lit
         | NAME ref_struct_fields -> struct_lit
         | "count" "(" NAME ":" qname "where" ref_expr ")" -> count_e
         | "sum" "(" NAME ":" qname "of" ref_expr ["where" ref_expr] ")" -> sum_e
         | "min" "(" ref_expr "," ref_expr ")" -> min_e
         | "max" "(" ref_expr "," ref_expr ")" -> max_e
         | "abs" "(" ref_expr ")" -> abs_e
         | "old" "(" ref_expr ")" -> old_e
         | NAME -> var
         | "(" ref_expr ")"
ref_struct_fields: "{" NAME ":" ref_expr ("," NAME ":" ref_expr)* ","? "}" -> struct_fields
ref_expr_list: ref_expr ("," ref_expr)* -> expr_list

requirements_def: "requirements" NAME "{" requirements_item* "}"
?requirements_item: implements_def | requirement_def | acceptance_def
                  | const_def | type_def | enum_def | struct_def
                  | state_def | init_def | req_action_def
                  | invariant_def | reachable_def | leadsto_def
implements_def: "implements" NAME "from" STRING "{" implements_item* "}"
?implements_item: map_def
requirement_def: "requirement" REQ_ID STRING "{" requirement_item* "}"
?requirement_item: req_action_def | invariant_def | reachable_def | leadsto_def
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
acceptance_expect: "expect" expr

CMPOP: "==" | "!=" | "<=" | ">=" | "<" | ">"
REQ_ID: /[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*/
_AND: /and\b/
_OR: /or\b/
_NOT: /not\b/
_IF: /if\b/
_ELSE: /else\b/
_FAIR: /fair\b/
NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
INT: /[0-9]+/
STRING: /"[^"]*"/
COMMENT: /\/\/[^\n]*/
%import common.WS
%ignore WS
%ignore COMMENT
"""


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
    _, p, q = node
    return binders, p, q


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

    def binder_typed(self, meta, v, ty, where=None):
        return ("binder_typed", v, ty, where)

    def binder_range(self, meta, v, lo, hi):
        return ("binder_range", v, lo, hi)

    def quant_forall(self, meta, b, e):
        return ("forall", b, e)

    def quant_forall_brace(self, meta, b, e):
        return ("forall", b, e)

    def quant_exists(self, meta, b, e):
        return ("exists", b, e)

    def quant_exists_brace(self, meta, b, e):
        return ("exists", b, e)

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

    def enum_member(self, meta, name):
        return name

    def field(self, meta, n, ty):
        return (n, ty)

    def state_def(self, meta, *decls):
        return ("state", [d for d in decls if d])

    def const_def(self, meta, n, e):
        return ("const", n, e)

    def type_def(self, meta, n, lo, hi):
        return ("type", n, lo, hi)

    def enum_def(self, meta, n, *members):
        return ("enum", n, [m for m in members if m])

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

    def reachable_def(self, meta, n, *rest):
        req_meta, e = None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            else:
                e = r
        return ("reachable", n, e, _loc(meta), req_meta)

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
        return ("action", name, list(exprs))

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

    def lt_implies(self, meta, p, q):
        return ("lt_implies", p, q)

    def lt_forall(self, meta, binder, body):
        return ("lt_forall", binder, body)

    def leadsto_def(self, meta, name, *rest):
        req_meta, body = None, None
        for r in rest:
            if isinstance(r, dict):
                req_meta = r
            else:
                body = r
        binders, p, q = _flatten_leadsto(body)
        return ("leadsto", name, binders, p, q, _loc(meta), req_meta)

    def start(self, meta, child):
        return child

    def spec_def(self, meta, name, *items):
        return ("spec", name, [i for i in items if i])

    def refinement_impl(self, meta, name):
        return ("impl", name)

    def refinement_abs(self, meta, name):
        return ("abs", name)

    def map_def(self, meta, name, binder=None, expr=None):
        if binder is not None:
            return ("map", name, binder, expr, _loc(meta))
        return ("map", name, None, expr, _loc(meta))

    def refinement_param(self, meta, name):
        return name

    def action_target(self, meta, child):
        return child

    def stutter_target(self, meta):
        return ("stutter",)

    def mapped_action_target(self, meta, name, *exprs):
        return ("action", name, list(exprs))

    def refinement_action(self, meta, name, *params_and_target):
        params = []
        target = None
        for p in params_and_target:
            if isinstance(p, str):
                params.append(p)
            else:
                target = p
        if target is None:
            raise ValueError("refinement action missing target")
        return ("action_map", name, params, target, _loc(meta))

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
        return ("acceptance_step", name, list(args), _loc(meta))

    def acceptance_expect(self, meta, expr):
        return ("acceptance_expect", expr, _loc(meta))

    def acceptance_def(self, meta, ac_id, text, *parts):
        steps = []
        expect = None
        for p in parts:
            if isinstance(p, tuple) and p[0] == "acceptance_step":
                steps.append(p)
            elif isinstance(p, tuple) and p[0] == "acceptance_expect":
                expect = p
        return ("acceptance", str(ac_id), text, steps, expect, _loc(meta))


PARSER = Lark(
    GRAMMAR,
    parser="earley",
    maybe_placeholders=True,
    propagate_positions=True,
)
